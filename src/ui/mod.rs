pub mod app;
pub mod brand;
pub mod components;
pub mod keys;
pub mod layout;
pub mod messages;
pub mod panels;
pub mod style;
pub mod text;
pub mod webdav_form;

/// Which file pane has focus.
///
/// It lives here rather than in `app` because both the app and the components
/// that render for it need to agree on the answer. The transfer direction is not
/// a separate fact: `start_copy` reads nothing but this to decide upload from
/// download, so the status bar must read nothing else either — a second
/// enumeration saying the same thing is a future divergence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivePane {
    Local,
    Remote,
}

impl ActivePane {
    pub fn is_local(self) -> bool {
        self == ActivePane::Local
    }
}
