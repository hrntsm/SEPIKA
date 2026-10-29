use std::io;
use std::mem::size_of;
use std::ptr::{null, null_mut};
use tokio::net::windows::named_pipe::{NamedPipeClient, NamedPipeServer, ServerOptions};
use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenLogonSid, SECURITY_ATTRIBUTES, TOKEN_GROUPS, TOKEN_QUERY,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_OVERLAPPED, FILE_GENERIC_READ, FILE_WRITE_ATTRIBUTES, FILE_WRITE_DATA,
    OPEN_EXISTING, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

const CLIENT_ACCESS: u32 = FILE_GENERIC_READ | FILE_WRITE_DATA | FILE_WRITE_ATTRIBUTES;

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn logon_sid() -> io::Result<String> {
    unsafe {
        let mut token = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut bytes = 0;
        GetTokenInformation(token, TokenLogonSid, null_mut(), 0, &mut bytes);
        let mut data = vec![0usize; (bytes as usize).div_ceil(size_of::<usize>())];
        let ok = GetTokenInformation(token, TokenLogonSid, data.as_mut_ptr().cast(), bytes, &mut bytes);
        let error = io::Error::last_os_error();
        CloseHandle(token);
        if ok == 0 {
            return Err(error);
        }
        let groups = &*data.as_ptr().cast::<TOKEN_GROUPS>();
        if groups.GroupCount != 1 {
            return Err(io::Error::other("ログオンSIDを一意に取得できません"));
        }
        let mut string = null_mut();
        if ConvertSidToStringSidW(groups.Groups[0].Sid, &mut string) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut length = 0;
        while *string.add(length) != 0 {
            length += 1;
        }
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(string, length));
        LocalFree(string.cast());
        Ok(sid)
    }
}

/// 同じログオンのクライアントだけを許可し、同名の既存パイプとリモート接続を拒否します。
pub fn create(name: &str) -> io::Result<NamedPipeServer> {
    let sddl = wide(&format!("D:P(A;;{CLIENT_ACCESS:#x};;;{})", logon_sid()?));
    unsafe {
        let mut descriptor = null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), 1, &mut descriptor, null_mut()) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let result = ServerOptions::new()
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .max_instances(1)
            .create_with_security_attributes_raw(name, (&mut attributes as *mut SECURITY_ATTRIBUTES).cast());
        LocalFree(descriptor);
        result
    }
}

/// パイプ作成権限と偽装権限を要求せず、指定GUIへ接続します。
pub fn connect(name: &str) -> io::Result<NamedPipeClient> {
    let name = wide(name);
    unsafe {
        let handle = CreateFileW(
            name.as_ptr(), CLIENT_ACCESS, 0, null(), OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION, null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        NamedPipeClient::from_raw_handle(handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn same_logon_round_trip_eof_missing_and_exclusive_creation() {
        let name = format!(r"\\.\pipe\sepika-test-{}", uuid::Uuid::new_v4());
        let mut server = create(&name).unwrap();
        assert!(create(&name).is_err());
        let mut client = connect(&name).unwrap();
        server.connect().await.unwrap();
        client.write_all("検討\n".as_bytes()).await.unwrap();
        let mut buffer = [0; 7];
        server.read_exact(&mut buffer).await.unwrap();
        assert_eq!(&buffer, "検討\n".as_bytes());
        server.write_all(b"accepted\n").await.unwrap();
        let mut response = [0; 9];
        client.read_exact(&mut response).await.unwrap();
        assert_eq!(&response, b"accepted\n");
        drop(client);
        assert_eq!(server.read(&mut buffer).await.unwrap(), 0);
        drop(server);
        assert!(connect(&name).is_err());
    }

    #[tokio::test]
    async fn dacl_excludes_create_instance_and_generic_write() {
        let name = format!(r"\\.\pipe\sepika-test-{}", uuid::Uuid::new_v4());
        let server = create(&name).unwrap();
        assert!(tokio::net::windows::named_pipe::ClientOptions::new().open(&name).is_err());
        assert!(ServerOptions::new().create(&name).is_err());
        let client = connect(&name).unwrap();
        drop(client);
        drop(server);
    }
}
