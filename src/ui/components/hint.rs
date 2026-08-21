/// Hint is a key/description pair, as advertised to the user.
///
/// It lives here rather than in `statusbar` because `keys` produces it: a binding
/// is the only thing that knows which key it answers to, so it is the only thing
/// entitled to say so. Anything that hardcodes a `Hint` for a bound key is a
/// second copy of the truth, and the status bar spent a while advertising `d` for
/// "download" while `d` deleted.
#[derive(Debug, Clone)]
pub struct Hint {
    pub key: String,
    pub desc: String,
}

impl Hint {
    /// For the handful of hints with no binding behind them (`j/k`, `tab`).
    pub fn new(key: &str, desc: &str) -> Self {
        Hint {
            key: key.to_string(),
            desc: desc.to_string(),
        }
    }
}
