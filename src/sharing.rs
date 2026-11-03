//! Sharing a file by link. LOKI gives apps no list of people, so an owner shares
//! with everyone who has access to Office Suite and has the link: opening it
//! (`/shared/{token}`) lets that person view or edit the file, and lists it
//! under "Shared with you". A new link cuts off everyone who came in through
//! the old one.
use crate::files::{Kind, owner};
use crate::ui::{Shell, display_name, now_ms, page};
use askama::Template;
use loki_sdk::{Database, Error, Request, Response, Result, Statement, params};
use serde::Deserialize;

fn not_found() -> Error {
    Error::not_found("This file does not exist, or it isn't yours.")
}

#[derive(Deserialize)]
struct Current {
    kind: Kind,
    title: String,
    role: Option<String>,
    token: Option<String>,
}
/// The signed-in owner's file `id` with its share, if any.
fn current(owner: &str, id: &str) -> Result<Current> {
    Database
        .fetch_optional(
            "SELECT f.kind,f.title,s.role,s.token FROM files f LEFT JOIN shares s ON s.file_id=f.id
             WHERE f.id=? AND f.owner=? AND f.trashed_at IS NULL",
            params![id, owner],
        )?
        .ok_or_else(not_found)
}

#[derive(Template)]
#[template(path = "share.html")]
struct SharePage {
    shell: Shell,
    id: String,
    title: String,
    href: String,
    noun: &'static str,
    /// "off", "view" or "edit".
    role: String,
    link: String,
}

/// `GET /files/{id}/share`: the owner's sharing settings.
pub fn settings(request: Request) -> Result<Response> {
    let owner = owner(&request)?;
    let id = request.param("id")?;
    let current = current(&owner, id)?;
    let tool = current.kind.tool();
    let origin = request.context().origin.trim_end_matches('/').to_string();
    page(Response::render(SharePage {
        shell: Shell::new(&request, Some(tool))?,
        href: format!("{}/{id}", tool.href),
        id: id.into(),
        title: current.title,
        noun: tool.noun,
        role: current.role.unwrap_or_else(|| "off".into()),
        link: current.token.map(|t| format!("{origin}/shared/{t}")).unwrap_or_default(),
    })?)
}

/// `POST /files/{id}/share` with `mode` = off, view or edit, and `reset` to make
/// a new link.
pub fn update(request: Request) -> Result<Response> {
    let user = request.context().require_user()?.clone();
    let id = request.param("id")?.to_string();
    let current = current(&user.id, &id)?;
    let form = request.form()?;
    let mode = form.required("mode")?;
    let reset = form.field("reset") == Some("1");
    let forget = Statement::new("DELETE FROM share_access WHERE file_id=?", params![id])?;
    let statements = match mode {
        "off" => vec![forget, Statement::new("DELETE FROM shares WHERE file_id=?", params![id])?],
        "view" | "edit" => {
            let name = display_name(&user);
            match current.token {
                None => vec![Statement::new(
                    "INSERT INTO shares(file_id,role,token,owner_name,shared_at) VALUES(?,?,?,?,?)",
                    params![id, mode, loki_sdk::random_id(), name, now_ms()],
                )?],
                Some(_) if reset => vec![
                    forget,
                    Statement::new(
                        "UPDATE shares SET role=?,token=?,owner_name=? WHERE file_id=?",
                        params![mode, loki_sdk::random_id(), name, id],
                    )?,
                ],
                Some(_) => vec![Statement::new(
                    "UPDATE shares SET role=?,owner_name=? WHERE file_id=?",
                    params![mode, name, id],
                )?],
            }
        }
        _ => return Err(Error::bad("Choose off, view or edit.")),
    };
    Database.transaction(&statements)?;
    Response::redirect(&format!("/files/{id}/share"))
}

/// `GET /shared/{token}`: opens a shared file and remembers it for the person.
pub fn open_link(request: Request) -> Result<Response> {
    let user = owner(&request)?;
    let token = request.param("token")?;
    #[derive(Deserialize)]
    struct Found {
        id: String,
        kind: Kind,
        owner: String,
    }
    let found: Found = Database
        .fetch_optional(
            "SELECT f.id,f.kind,f.owner FROM shares s JOIN files f ON f.id=s.file_id
             WHERE s.token=? AND f.trashed_at IS NULL",
            params![token],
        )?
        .ok_or_else(|| Error::not_found("This link doesn't work any more. Ask the owner for a new one."))?;
    if found.owner != user {
        Database.execute(
            "INSERT INTO share_access(file_id,user_id,token,opened_at) VALUES(?,?,?,?)
             ON CONFLICT(file_id,user_id) DO UPDATE SET token=excluded.token,opened_at=excluded.opened_at",
            params![found.id, user, token, now_ms()],
        )?;
    }
    Response::redirect(&format!("{}/{}", found.kind.tool().href, found.id))
}

/// `POST /shared/{id}/forget`: takes a shared file off the person's list. The
/// link still works if they open it again.
pub fn forget(request: Request) -> Result<Response> {
    let user = owner(&request)?;
    Database.execute(
        "DELETE FROM share_access WHERE file_id=? AND user_id=?",
        params![request.param("id")?, user],
    )?;
    Response::redirect("/")
}

#[cfg(test)]
mod tests {
    use crate::test_host::{context, host};
    use loki_sdk::{Method, testing};
    use serde_json::json;

    #[test]
    fn only_the_owner_changes_sharing() {
        let (log, fake) = host(|_| json!({"rows": [], "affected": 0}));
        let request = testing::request(Method::Post, "/files/f-1/share", context(true));
        let error = testing::with_host(fake, || crate::router().handle(request).unwrap_err());
        assert_eq!(error.status(), 404);
        assert_eq!(log.borrow()[0]["params"], json!(["f-1", "user-1"]));
    }

    #[test]
    fn opening_a_link_remembers_it_for_others() {
        let (log, fake) = host(|sql| {
            if sql.starts_with("SELECT f.id,f.kind,f.owner") {
                json!({"rows": [{"id": "f-1", "kind": "sheet", "owner": "someone-else"}], "affected": 0})
            } else {
                json!({"rows": [], "affected": 1})
            }
        });
        let request = testing::request(Method::Get, "/shared/tok-1", context(true));
        let response = testing::with_host(fake, || crate::router().handle(request).unwrap());
        assert_eq!(response.headers["location"], "/sheets/f-1");
        let log = log.borrow();
        assert!(log[1]["sql"].as_str().unwrap().starts_with("INSERT INTO share_access"));
        assert_eq!(log[1]["params"].as_array().unwrap()[..3], [json!("f-1"), json!("user-1"), json!("tok-1")]);
    }

    #[test]
    fn dead_links_are_not_found() {
        let (_, fake) = host(|_| json!({"rows": [], "affected": 0}));
        let request = testing::request(Method::Get, "/shared/old", context(true));
        let error = testing::with_host(fake, || crate::router().handle(request).unwrap_err());
        assert_eq!(error.status(), 404);
    }

    fn shared_doc(role: &'static str) -> impl Fn(&str) -> serde_json::Value {
        move |sql: &str| {
            if sql.starts_with("SELECT f.id,f.title") {
                json!({"rows": [{
                    "id": "d-1", "title": "Plan", "version": 3, "info": "", "updated_at": 0, "trashed_at": null,
                    "owner": "ada", "role": role, "owner_name": "Ada", "body": r#"{"blocks":[]}"#
                }], "affected": 0})
            } else if sql.starts_with("SELECT f.owner,s.role") {
                json!({"rows": [{"owner": "ada", "role": role, "owner_name": "Ada"}], "affected": 0})
            } else {
                json!({"rows": [], "affected": 0})
            }
        }
    }

    #[test]
    fn viewers_get_the_read_only_page_and_cannot_save() {
        let (log, fake) = host(shared_doc("view"));
        let editor = testing::request(Method::Get, "/docs/d-1", context(true));
        let save = testing::request(Method::Post, "/api/docs/d-1", context(true));
        testing::with_host(fake, || {
            let response = crate::router().handle(editor).unwrap();
            assert_eq!(response.headers["location"], "/docs/d-1/view");
            assert_eq!(crate::router().handle(save).unwrap_err().status(), 403);
        });
        // Access needs the share's current token, checked in the query.
        let sql = log.borrow()[0]["sql"].as_str().unwrap().to_string();
        assert!(sql.contains("a.token=s.token") && sql.contains("f.trashed_at IS NULL"));
        assert_eq!(log.borrow()[0]["params"], json!(["d-1", "doc", "user-1", "user-1"]));
    }

    #[test]
    fn editors_open_the_editor_with_who_shared_it() {
        let (_, fake) = host(shared_doc("edit"));
        let editor = testing::request(Method::Get, "/docs/d-1", context(true));
        let response = testing::with_host(fake, || crate::router().handle(editor).unwrap());
        assert_eq!(response.status, 200);
        let page = String::from_utf8(response.body).unwrap();
        assert!(page.contains("Shared by Ada · can edit"));
        assert!(!page.contains("/files/d-1/versions") && !page.contains("/files/d-1/share"));
    }

    #[test]
    fn images_in_shared_documents_load_for_people_who_can_open_them() {
        let image = "0f8c2c4e-1b2a-4c3d-9e8f-001122334455.png";
        let (log, fake) = host(|sql| {
            if sql.starts_with("SELECT owner FROM images") {
                json!({"rows": [{"owner": "ada"}], "affected": 0})
            } else {
                json!({"rows": [{"ok": 1}], "affected": 0})
            }
        });
        let request = testing::request(Method::Get, &format!("/docs/images/{image}"), context(true));
        let response = testing::with_host(fake, || crate::router().handle(request).unwrap());
        assert_eq!(response.status, 200);
        let log = log.borrow();
        assert!(log[1]["sql"].as_str().unwrap().contains("instr(f.body,?)"));
        assert_eq!(log[1]["params"], json!([image, "user-1", "user-1"]));

        let (_, fake) = host(|sql| {
            if sql.starts_with("SELECT owner FROM images") {
                json!({"rows": [{"owner": "ada"}], "affected": 0})
            } else {
                json!({"rows": [], "affected": 0})
            }
        });
        let request = testing::request(Method::Get, &format!("/docs/images/{image}"), context(true));
        let error = testing::with_host(fake, || crate::router().handle(request).unwrap_err());
        assert_eq!(error.status(), 404);
    }
}
