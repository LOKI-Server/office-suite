use loki_sdk::{Request, Response, Result, User};

/// One tool in the suite, such as Docs.
pub struct Tool {
    pub name: &'static str,
    /// Where the tool's own home page lives, such as `/docs`. Its files open at
    /// `{href}/{id}`, read-only at `{href}/{id}/view`, and new ones are made by
    /// posting to `{href}/new`.
    pub href: &'static str,
    /// What one file is called, such as "document".
    pub noun: &'static str,
    /// The icon template under `templates/icons/`, without `.svg`.
    pub icon: &'static str,
    /// Shown in lists in place of an empty excerpt.
    pub empty: &'static str,
    /// Download formats: a label and the file name under `{href}/{id}/`.
    pub exports: &'static [(&'static str, &'static str)],
}

/// Page chrome for templates extending `base.html`, which need a `shell` field.
pub struct Shell {
    /// The tool the page belongs to; `None` on the suite's home page.
    pub tool: Option<&'static Tool>,
    /// The signed-in person's display name.
    pub user: String,
    pub csrf: String,
    /// LOKI's dashboard, when the host reports it.
    pub dashboard: String,
}
impl Shell {
    pub fn new(request: &Request, tool: Option<&'static Tool>) -> Result<Self> {
        let context = request.context();
        Ok(Self {
            tool,
            user: display_name(context.require_user()?),
            csrf: context.csrf_token().into(),
            dashboard: context.dashboard_url.clone(),
        })
    }
}

/// The name to show for a person: their profile name, else their username.
fn display_name(user: &User) -> String {
    let name = user.name.trim();
    if name.is_empty() {
        user.username.clone()
    } else {
        name.into()
    }
}

/// Hardens an HTML page: scripts only from the app itself, no framing, no caching
/// of personal content.
pub fn page(response: Response) -> Result<Response> {
    response
        .header(
            "content-security-policy",
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
             img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'none'; \
             form-action 'self'; frame-ancestors 'none'",
        )?
        .header("cache-control", "no-store")
}
