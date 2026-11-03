//! Housekeeping LOKI runs once a day (`schedules` in Cargo.toml): deleting
//! files that have been in the trash too long, then images no file or saved
//! version uses any more.
use crate::ui::now_ms;
use crate::{docs, files};
use loki_sdk::{Database, ErrorKind, Request, Response, Result, params};
use serde::Deserialize;

/// The job's name in Cargo.toml and its route, `POST /_loki/schedule/cleanup`.
pub const NAME: &str = "cleanup";
const DAY_MS: i64 = 86_400_000;
/// An image nothing mentions is kept this long after it was recorded, so one
/// added moments before its document was saved is never lost.
const IMAGE_GRACE_MS: i64 = DAY_MS;
/// Images deleted per run, to stay within the request time limit; the rest
/// wait for the next day.
const IMAGE_BATCH: i64 = 500;

pub fn run(_: Request) -> Result<Response> {
    let now = now_ms();
    let files = files::purge_all_expired(now)?;
    let images = remove_unused_images(now)?;
    loki_sdk::log(format!("cleanup: deleted {files} expired files and {images} unused images"))?;
    Ok(Response::text("done"))
}

/// Images are only ever used by their owner's documents, so a name that none
/// of those files or versions contain is unused.
fn remove_unused_images(now: i64) -> Result<usize> {
    #[derive(Deserialize)]
    struct Unused {
        name: String,
        owner: String,
    }
    let unused: Vec<Unused> = Database.fetch_all(
        "SELECT name,owner FROM images i WHERE created_at<?
           AND NOT EXISTS (SELECT 1 FROM files f WHERE f.owner=i.owner AND f.kind='doc' AND instr(f.body,i.name)>0)
           AND NOT EXISTS (SELECT 1 FROM file_versions v WHERE v.owner=i.owner AND instr(v.body,i.name)>0)
         LIMIT ?",
        params![now - IMAGE_GRACE_MS, IMAGE_BATCH],
    )?;
    for image in &unused {
        match docs::delete_image(&image.owner, &image.name) {
            // Already gone, or recorded from a document that never uploaded it.
            Err(error) if error.kind() != ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        Database.execute(
            "DELETE FROM images WHERE name=? AND owner=?",
            params![image.name, image.owner],
        )?;
    }
    Ok(unused.len())
}

#[cfg(test)]
mod tests {
    use crate::test_host::context;
    use loki_sdk::{Method, testing};
    use serde_json::{Value, json};
    use std::{cell::RefCell, rc::Rc};

    #[test]
    fn deletes_expired_files_then_unused_images() {
        let calls: Rc<RefCell<Vec<(String, Value)>>> = Rc::default();
        let seen = calls.clone();
        let fake = move |operation: &str, input: &str| -> Result<String, String> {
            let input: Value = serde_json::from_str(input).unwrap();
            seen.borrow_mut().push((operation.into(), input.clone()));
            match operation {
                "database" => {
                    let results: Vec<Value> = input["statements"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|s| {
                            let sql = s["sql"].as_str().unwrap();
                            if sql.starts_with("SELECT name,owner") {
                                json!({"rows": [
                                    {"name": "a.png", "owner": "u1"},
                                    {"name": "gone.png", "owner": "u2"}
                                ], "affected": 0})
                            } else {
                                json!({"rows": [], "affected": 3})
                            }
                        })
                        .collect();
                    Ok(Value::Array(results).to_string())
                }
                "delete-file" if input["path"] == "images/u2/gone.png" => {
                    Err(json!({"loki_error": 1, "status": 404, "message": "No such file."}).to_string())
                }
                "delete-file" | "log" => Ok("null".into()),
                other => Err(format!("unexpected host call {other}")),
            }
        };
        // Scheduled jobs run without a signed-in user.
        let request = testing::request(Method::Post, "/_loki/schedule/cleanup", context(false));
        let response = testing::with_host(fake, || crate::router().handle(request).unwrap());
        assert_eq!(response.status, 200);
        let calls = calls.borrow();
        let sql = |i: usize| calls[i].1["statements"][0]["sql"].as_str().unwrap().to_string();
        assert!(sql(0).starts_with("DELETE FROM file_versions"));
        assert!(calls[0].1["statements"][1]["sql"].as_str().unwrap().starts_with("DELETE FROM files"));
        assert!(sql(1).starts_with("SELECT name,owner FROM images"));
        assert_eq!(calls[2], ("delete-file".into(), json!({"path": "images/u1/a.png"})));
        assert_eq!(calls[3].1["statements"][0]["params"], json!(["a.png", "u1"]));
        assert_eq!(calls[4].1["path"], "images/u2/gone.png");
        assert_eq!(calls[5].1["statements"][0]["params"], json!(["gone.png", "u2"]));
        assert_eq!(calls[6].0, "log");
        assert!(calls[6].1["message"].as_str().unwrap().contains("3 expired files and 2 unused images"));
    }
}
