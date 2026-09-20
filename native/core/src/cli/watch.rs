use super::{
    args::{ApprovalMode, TaskOptions},
    backend::Backend,
    Outcome,
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    io::{IsTerminal, Write},
    time::Duration,
};

/// Strip terminal control sequences from model text and external tool output.
/// JSON output remains lossless because serde escapes the control characters.
pub fn plain(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect()
}
pub async fn interrupted(parent: Option<u32>) {
    #[cfg(unix)]
    {
        if let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! {_=tokio::signal::ctrl_c()=>{},_=terminate.recv()=>{},_=crate::lifecycle::parent_exited(parent)=>{}}
            return;
        }
    }
    tokio::select! {_=tokio::signal::ctrl_c()=>{},_=crate::lifecycle::parent_exited(parent)=>{}}
}
async fn line() -> Result<String> {
    ensure!(
        std::io::stdin().is_terminal(),
        "Interactive approval requires a terminal on stdin"
    );
    // Temporarily use nonblocking reads so Ctrl-C can drop this future without
    // leaving a blocking stdin task that prevents runtime shutdown.
    // SAFETY: fcntl operates on the existing stdin descriptor; no pointer is used.
    let old = unsafe { libc::fcntl(libc::STDIN_FILENO, libc::F_GETFL) };
    ensure!(old >= 0, "Could not inspect terminal input");
    struct Restore(i32);
    impl Drop for Restore {
        fn drop(&mut self) {
            unsafe {
                libc::fcntl(libc::STDIN_FILENO, libc::F_SETFL, self.0);
            }
        }
    }
    let _restore = Restore(old);
    ensure!(
        unsafe { libc::fcntl(libc::STDIN_FILENO, libc::F_SETFL, old | libc::O_NONBLOCK) } >= 0,
        "Could not prepare terminal input"
    );
    let mut bytes = Vec::new();
    loop {
        let mut byte = 0u8;
        // SAFETY: byte is a valid writable one-byte buffer for the duration.
        let count = unsafe { libc::read(libc::STDIN_FILENO, (&mut byte as *mut u8).cast(), 1) };
        if count == 0 {
            return Ok(String::new());
        }
        if count == 1 {
            if byte == b'\n' {
                return Ok(String::from_utf8_lossy(&bytes).trim().into());
            }
            ensure!(bytes.len() < 8000, "Approval response is too long");
            bytes.push(byte);
        } else {
            let error = std::io::Error::last_os_error();
            if !matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
            ) {
                return Err(error.into());
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    }
}
async fn approve(backend: &Backend, approval: &Value) -> Result<()> {
    errln!(
        "\nApproval required: {}\n{}\n{}",
        plain(approval["tool"].as_str().unwrap_or("tool")),
        plain(approval["command"].as_str().unwrap_or("")),
        plain(approval["reason"].as_str().unwrap_or(""))
    );
    err!("Allow this exact operation? [y/N] ");
    std::io::stderr().flush()?;
    let answer = line().await?;
    let result=backend.call("POST",format!("/api/approvals/{}",approval["id"].as_str().context("Approval ID missing")?),json!({"session_id":approval["session_id"],"decision":if matches!(answer.to_ascii_lowercase().as_str(),"y"|"yes"){"approve"}else{"deny"}})).await;
    if let Err(error) = result {
        errln!("Approval was not applied: {}", plain(&format!("{error:#}")));
    }
    Ok(())
}
pub async fn job(
    backend: &Backend,
    initial: Value,
    options: &TaskOptions,
    json_output: bool,
    owns_job: bool,
) -> Result<Outcome> {
    let id = initial["id"].as_str().context("Job ID missing")?.to_owned();
    let task_id = initial["task_id"].as_str().unwrap_or("").to_owned();
    let sid = initial["session_id"]
        .as_str()
        .context("Session ID missing")?
        .to_owned();
    let interactive = options.interactive
        || (!json_output
            && !options.events
            && std::io::stdin().is_terminal()
            && std::io::stderr().is_terminal());
    if options.interactive {
        ensure!(
            std::io::stdin().is_terminal(),
            "--interactive requires a terminal on stdin"
        );
    }
    // A freshly started task returns its starting cursor; an existing job may
    // already hold its final cursor. Replay it from history, filtering task IDs,
    // so `jobs --watch` also shows the saved output of completed jobs.
    let mut cursor = if owns_job {
        initial["event_cursor"].as_i64().unwrap_or(0)
    } else {
        0
    };
    let mut streamed = HashSet::new();
    let mut announced = HashSet::new();
    let mut text_open = false;
    let signal = interrupted(backend.parent);
    tokio::pin!(signal);
    let result = async {
        loop {
            let page = backend
                .call(
                    "GET",
                    format!("/api/jobs/{id}/events?after={cursor}&limit=512"),
                    Value::Null,
                )
                .await?;
            let rows = page["events"].as_array().context("Job events missing")?;
            for event in rows {
                cursor = cursor.max(event["id"].as_i64().unwrap_or(0));
                if event["task_id"].as_str() != Some(task_id.as_str()) {
                    continue;
                }
                if options.events {
                    outln!(
                        "{}",
                        serde_json::to_string(&json!({"type":"event","event":event}))?
                    );
                } else if !json_output {
                    let payload = &event["payload"];
                    match event["type"].as_str().unwrap_or("") {
                        "model.stream" => {
                            streamed
                                .insert(payload["message_id"].as_str().unwrap_or("").to_owned());
                            out!("{}", plain(payload["text"].as_str().unwrap_or("")));
                            std::io::stdout().flush()?;
                            text_open = true;
                        }
                        "model.delta" => {
                            if !streamed.contains(payload["message_id"].as_str().unwrap_or("")) {
                                outln!("{}", plain(payload["text"].as_str().unwrap_or("")));
                            } else if text_open {
                                outln!();
                                text_open = false;
                            }
                        }
                        "tool.started" => {
                            if text_open {
                                outln!();
                                text_open = false;
                            }
                            errln!(
                                "→ {} {}",
                                plain(payload["tool"].as_str().unwrap_or("tool")),
                                plain(
                                    payload["arguments"]["command"]
                                        .as_str()
                                        .or(payload["arguments"]["path"].as_str())
                                        .unwrap_or("")
                                )
                            );
                        }
                        "routing.selected" | "routing.fallback" => errln!(
                            "Model: {} · {}",
                            plain(payload["model_name"].as_str().unwrap_or("")),
                            plain(payload["purpose"].as_str().unwrap_or(""))
                        ),
                        "workflow.selected" => errln!(
                            "Workflow: {} ({})",
                            plain(payload["name"].as_str().unwrap_or("")),
                            plain(payload["path"].as_str().unwrap_or(""))
                        ),
                        _ => {}
                    }
                }
            }
            let current = &page["job"];
            if !matches!(
                current["status"].as_str(),
                Some("queued" | "running" | "cancelling")
            ) && cursor >= current["event_cursor"].as_i64().unwrap_or(0)
            {
                return Ok(Outcome {
                    code: if current["status"] == "completed" {
                        0
                    } else {
                        1
                    },
                    value: current.clone(),
                    raw: (!json_output && !options.events).then(|| {
                        format!(
                            "{}{} · conversation {} · job {}\n",
                            if text_open { "\n" } else { "" },
                            current["status"].as_str().unwrap_or(""),
                            sid,
                            id
                        )
                    }),
                });
            }
            if rows.len() == 512 {
                continue;
            }
            let approvals = backend
                .call(
                    "GET",
                    format!("/api/approvals?session_id={sid}"),
                    Value::Null,
                )
                .await?;
            for approval in approvals["approvals"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|approval| approval["task_id"] == task_id)
            {
                if interactive {
                    approve(backend, approval).await?;
                } else if owns_job && matches!(options.approval, ApprovalMode::Cancel) {
                    let stopped = backend
                        .call("POST", format!("/api/jobs/{id}/cancel"), json!({}))
                        .await?;
                    return Ok(Outcome {
                        code: 2,
                        value: json!({"status":"needs_approval","message":"Task stopped because this non-interactive invocation cannot grant tool approval. Use --interactive in a terminal, or --approval wait and approve through the desktop/approvals command.","approval":approval,"job":stopped}),
                        raw: None,
                    });
                } else if announced.insert(approval["id"].as_str().unwrap_or("").to_owned())
                    && !json_output
                    && !options.events
                {
                    errln!(
                        "Waiting for tool approval: {}",
                        plain(approval["id"].as_str().unwrap_or(""))
                    );
                }
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    };
    let outcome: Result<Outcome> = tokio::select! {
        outcome=result=>outcome,
        _=&mut signal=>{
            if owns_job {let stopped=backend.call("POST",format!("/api/jobs/{id}/cancel"),json!({})).await?;Ok(Outcome{code:130,value:stopped,raw:None})}
            else {Ok(Outcome{code:130,value:json!({"status":"detached","job_id":id,"message":"Stopped watching; the existing job continues."}),raw:None})}
        }
    };
    if let Err(error) = outcome {
        if owns_job {
            match tokio::time::timeout(
                Duration::from_secs(20),
                backend.call("POST", format!("/api/jobs/{id}/cancel"), json!({})),
            )
            .await
            {
                Ok(Ok(_)) => {
                    return Err(error.context("Task cancelled after its CLI observer failed"))
                }
                _ => {
                    return Err(error.context(format!(
                        "Could not confirm cancellation; inspect job {id} in the desktop or CLI"
                    )))
                }
            }
        }
        return Err(error);
    }
    outcome
}
pub async fn goal(
    backend: &Backend,
    id: &str,
    interactive: bool,
    json_output: bool,
) -> Result<Outcome> {
    if interactive {
        ensure!(
            std::io::stdin().is_terminal(),
            "--interactive requires a terminal on stdin"
        );
    }
    let signal = interrupted(backend.parent);
    tokio::pin!(signal);
    let progress = async {
        let mut previous = String::new();
        loop {
            let goal = backend
                .call("GET", format!("/api/goals/{id}"), Value::Null)
                .await?;
            if goal["running"] != true {
                return Ok(Outcome {
                    code: if goal["status"] == "completed" { 0 } else { 1 },
                    value: goal,
                    raw: None,
                });
            }
            let detail = goal["run_detail"].as_str().unwrap_or("");
            if detail != previous && !json_output {
                errln!("{}", plain(detail));
                previous = detail.into();
            }
            if let Some(sid) = goal["session_id"].as_str() {
                let approvals = backend
                    .call(
                        "GET",
                        format!("/api/approvals?session_id={sid}"),
                        Value::Null,
                    )
                    .await?;
                let active = if let Some(job) = goal["job_id"].as_str() {
                    backend
                        .call("GET", format!("/api/jobs/{job}"), Value::Null)
                        .await?
                } else {
                    Value::Null
                };
                for approval in approvals["approvals"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|approval| {
                        !active["task_id"].is_null() && approval["task_id"] == active["task_id"]
                    })
                {
                    if interactive || (!json_output && std::io::stdin().is_terminal()) {
                        approve(backend, approval).await?;
                    } else {
                        let paused = backend
                            .call("POST", format!("/api/goals/{id}/pause"), json!({}))
                            .await?;
                        return Ok(Outcome {
                            code: 2,
                            value: json!({"status":"needs_approval","goal":paused,"approval":approval}),
                            raw: None,
                        });
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    };
    let outcome: Result<Outcome> = tokio::select! {result=progress=>result,_=&mut signal=>Ok(Outcome{code:130,value:backend.call("POST",format!("/api/goals/{id}/pause"),json!({})).await?,raw:None})};
    if let Err(error) = outcome {
        return match tokio::time::timeout(
            Duration::from_secs(20),
            backend.call("POST", format!("/api/goals/{id}/pause"), json!({})),
        )
        .await
        {
            Ok(Ok(_)) => Err(error.context("Goal paused after its CLI observer failed")),
            _ => Err(error.context(format!(
                "Could not confirm pause; inspect goal {id} in the desktop or CLI"
            ))),
        };
    }
    outcome
}
