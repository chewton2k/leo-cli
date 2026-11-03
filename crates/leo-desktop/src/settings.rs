#[derive(Clone, Copy, Debug)]
pub enum PasteMethod {
    CtrlV,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardHandling {
    Restore,
    CopyToClipboard,
}
