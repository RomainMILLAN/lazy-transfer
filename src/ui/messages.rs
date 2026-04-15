/// Action represents commands that the UI can trigger.
#[derive(Debug)]
pub enum Action {
    None,
    Quit,
    InputSubmit(String),
    InputCancel,
    Refresh,
    SwitchPane,
}
