//! A fake LOKI host for native handler tests.
use loki_sdk::Context;
use serde_json::{Value, json};
use std::{cell::RefCell, rc::Rc};

pub type Log = Rc<RefCell<Vec<Value>>>;

pub fn context(signed_in: bool) -> Context {
    serde_json::from_value(json!({
        "app_id": "office",
        "origin": "http://office.localhost",
        "issuer": "http://localhost",
        "csrf": "token",
        "user": signed_in.then(|| json!({
            "id": "user-1", "username": "ada", "name": "Ada", "email": "", "roles": ["access"]
        })),
    }))
    .unwrap()
}

/// A host whose database answers each statement with `answer(sql)` and records
/// every statement it receives. Forms carry `title = "  New name "`.
pub fn host(
    answer: impl Fn(&str) -> Value + 'static,
) -> (Log, impl FnMut(&str, &str) -> Result<String, String>) {
    let log: Log = Rc::default();
    let seen = log.clone();
    let fake = move |operation: &str, input: &str| {
        let input: Value = serde_json::from_str(input).unwrap();
        match operation {
            "database" => {
                let results: Vec<Value> = input["statements"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|statement| {
                        seen.borrow_mut().push(statement.clone());
                        answer(statement["sql"].as_str().unwrap())
                    })
                    .collect();
                Ok(Value::Array(results).to_string())
            }
            "form" => Ok(json!({"fields": {"title": "  New name "}, "files": []}).to_string()),
            "log" => Ok("null".into()),
            other => Err(format!("unexpected host call {other}")),
        }
    };
    (log, fake)
}
