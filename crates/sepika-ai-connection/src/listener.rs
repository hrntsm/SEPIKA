use crate::{
    pipe,
    protocol::{read_frame, write_frame, Request, Response},
};
use std::{io, sync::mpsc};
use tokio::{io::BufReader, sync::oneshot};

pub struct Incoming {
    pub connection: String,
    pub request: Request,
    pub response: oneshot::Sender<Response>,
}
pub enum Event {
    Request(Box<Incoming>),
    Disconnected(String),
}
pub struct Listener {
    pub name: String,
    pub events: mpsc::Receiver<Event>,
    stop: Option<oneshot::Sender<()>>,
}
impl Drop for Listener {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}
impl Listener {
    pub fn start() -> io::Result<Self> {
        let name = format!(r"\\.\pipe\sepika-gui-{}", uuid::Uuid::new_v4());
        let (events_tx, events) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (stop, stopped) = oneshot::channel();
        let endpoint = name.clone();
        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Runtime::new() {
                Ok(r) => r,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            runtime.block_on(async move {
                let server = match pipe::create(&endpoint) { Ok(s)=>s, Err(e)=> { let _=ready_tx.send(Err(e)); return; } };
                let _=ready_tx.send(Ok(()));
                tokio::select! { _ = stopped => {}, _ = async move {
                    let mut server = server;
                    let connection = uuid::Uuid::new_v4().to_string();
                    if server.connect().await.is_ok() {
                        let (read,mut write)=tokio::io::split(&mut server);
                        let mut reader=BufReader::new(read);
                        loop {
                            let request = match read_frame::<Request>(&mut reader).await {
                                Ok(Some(r))=>r, Ok(None)=>break,
                                Err(e)=>{ let _=write_frame(&mut write,&Response::rejected(String::new(),"invalid_frame",&e.to_string())).await; break; }
                            };
                            let (response,rx)=oneshot::channel();
                            if events_tx.send(Event::Request(Box::new(Incoming {connection:connection.clone(),request,response}))).is_err() { return; }
                            let Ok(response)=rx.await else { return; };
                            if let Err(e)=write_frame(&mut write,&response).await {
                                if e.kind()==io::ErrorKind::InvalidData && write_frame(&mut write,&Response::rejected(response.id,"response_too_large",&e.to_string())).await.is_ok() {continue;}
                                break;
                            }
                        }
                        drop(reader);
                        drop(write);
                    }
                    drop(server);
                    let _=events_tx.send(Event::Disconnected(connection));
                } => {} }
            });
        });
        ready_rx
            .recv()
            .map_err(|_| io::Error::other("接続スレッドが終了しました"))??;
        Ok(Self {
            name,
            events,
            stop: Some(stop),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::Duration;

    #[tokio::test]
    async fn disconnected_endpoint_is_retired_and_new_endpoints_preserve_response_ids() {
        let mut previous = None;
        for client in 0..3 {
            let listener = Listener::start().unwrap();
            let name = listener.name.clone();
            assert_ne!(previous.as_ref(), Some(&name));
            if let Some(previous) = &previous {
                assert!(pipe::connect(previous).is_err());
            }
            let worker = std::thread::spawn(move || {
                let mut connection = None;
                for sequence in 0..3 {
                    let event = listener
                        .events
                        .recv_timeout(Duration::from_secs(5))
                        .unwrap();
                    let Event::Request(incoming) = event else {
                        panic!("client {client} sequence {sequence}: premature disconnect");
                    };
                    let expected = format!("client-{client}-request-{sequence}");
                    assert_eq!(incoming.request.id, expected);
                    assert_eq!(
                        connection.get_or_insert(incoming.connection.clone()),
                        &incoming.connection
                    );
                    incoming
                        .response
                        .send(Response::accepted(
                            incoming.request.id,
                            json!({"client": client, "sequence": sequence}),
                        ))
                        .unwrap();
                }
                let Event::Disconnected(disconnected) = listener
                    .events
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
                else {
                    panic!("missing disconnect");
                };
                assert_eq!(Some(disconnected), connection);
            });
            let mut stream = BufReader::new(pipe::connect(&name).unwrap());
            for sequence in 0..3 {
                let id = format!("client-{client}-request-{sequence}");
                let request = serde_json::from_value::<Request>(
                    json!({"version": 1, "id": id, "operation": "connect"}),
                )
                .unwrap();
                write_frame(stream.get_mut(), &request).await.unwrap();
                let response = tokio::time::timeout(
                    Duration::from_secs(5),
                    read_frame::<Response>(&mut stream),
                )
                .await
                .unwrap()
                .unwrap()
                .unwrap();
                assert_eq!(response.id, id);
                assert_eq!(
                    response.result.unwrap(),
                    json!({"client": client, "sequence": sequence})
                );
            }
            drop(stream);
            tokio::task::spawn_blocking(move || worker.join().unwrap())
                .await
                .unwrap();
            assert!(pipe::connect(&name).is_err());
            previous = Some(name);
        }
    }
}
