//! GUIが許可した検討へのローカル接続。

#[cfg(windows)]
pub mod pipe;
#[cfg(windows)]
pub mod listener;
pub mod protocol;
