//! GUIが許可した検討へのローカル接続。

#[cfg(windows)]
pub mod listener;
#[cfg(windows)]
pub mod pipe;
pub mod protocol;
