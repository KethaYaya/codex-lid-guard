//! Tiny deterministic app-server peer for lifecycle tests. Never runs user commands.
use std::io::{self, BufRead, Write};
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::Duration;

fn emit(value: &str) {
    println!("{value}");
    io::stdout().flush().unwrap();
}
fn reply(id: &str, result: &str) {
    emit(&format!("{{\"id\":{id},\"result\":{result}}}"));
}
fn complete(status: &str) {
    emit(&format!(
        "{{\"method\":\"turn/completed\",\"params\":{{\"threadId\":\"test-thread\",\"turn\":{{\"id\":\"test-turn\",\"status\":\"{status}\"}}}}}}"
    ));
}
fn main() {
    if std::env::args().any(|arg| arg == "--child") {
        loop {
            std::thread::sleep(Duration::from_secs(10));
        }
    }
    let mut turns = 0;
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        let id = line
            .split("\"id\":")
            .nth(1)
            .unwrap_or("0")
            .split([',', '}'])
            .next()
            .unwrap();
        if line.contains("\"method\":\"initialize\"") {
            let child = Command::new(std::env::current_exe().unwrap())
                .arg("--child")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(0x08000000)
                .spawn()
                .unwrap();
            std::fs::write("fixture-child.pid", child.id().to_string()).unwrap();
            reply(id, "{}");
        } else if line.contains("\"method\":\"account/read\"") {
            reply(
                id,
                "{\"account\":{\"type\":\"chatgpt\"},\"requiresOpenaiAuth\":true}",
            );
        } else if line.contains("\"method\":\"thread/start\"") {
            assert!(line.contains("\"sandbox\":\"workspace-write\""));
            assert!(line.contains("\"approvalPolicy\":\"on-request\""));
            assert!(line.contains("\"approvalsReviewer\":\"user\""));
            assert!(line.contains("\"historyMode\":\"legacy\""));
            assert!(line.contains("\"threadSource\":\"user\""));
            reply(id, "{\"thread\":{\"id\":\"test-thread\"}}");
        } else if line.contains("\"method\":\"turn/start\"") {
            turns += 1;
            reply(
                id,
                "{\"turn\":{\"id\":\"test-turn\",\"status\":\"inProgress\"}}",
            );
            emit(
                "{\"method\":\"turn/started\",\"params\":{\"threadId\":\"test-thread\",\"turn\":{\"id\":\"test-turn\"}}}",
            );
            if line.contains("crash-fixture") {
                std::process::exit(3);
            }
            if line.contains("hang-fixture") {
                complete("completed");
                std::thread::sleep(Duration::from_secs(300));
                continue;
            }
            if line.contains("wait-fixture") {
                continue;
            }
            if turns == 1 {
                emit(
                    "{\"method\":\"turn/completed\",\"params\":{\"threadId\":\"test-thread\",\"turn\":{\"id\":\"old-turn\",\"status\":\"completed\"}}}",
                );
                emit(
                    "{\"id\":\"unsupported\",\"method\":\"item/permissions/requestApproval\",\"params\":{\"threadId\":\"test-thread\",\"turnId\":\"test-turn\"}}",
                );
                emit(
                    "{\"id\":\"approve-1\",\"method\":\"item/commandExecution/requestApproval\",\"params\":{\"threadId\":\"test-thread\",\"turnId\":\"test-turn\",\"itemId\":\"cmd\",\"command\":\"echo fixture only\",\"availableDecisions\":[\"accept\",\"decline\",\"cancel\"]}}",
                );
            } else {
                emit(
                    "{\"method\":\"item/completed\",\"params\":{\"threadId\":\"test-thread\",\"turnId\":\"test-turn\",\"item\":{\"id\":\"followup\",\"type\":\"agentMessage\",\"text\":\"Follow-up finished.\"}}}",
                );
                complete("completed");
            }
        } else if line.contains("\"method\":\"turn/steer\"") {
            reply(id, "{\"turnId\":\"test-turn\"}");
        } else if line.contains("\"method\":\"turn/interrupt\"") {
            reply(id, "{}");
            complete("interrupted");
        } else if line.contains("\"id\":\"unsupported\"") {
            assert!(line.contains("-32601"));
        } else if line.contains("\"id\":\"approve-1\"") {
            if line.contains("\"decision\":\"accept\"") {
                emit(
                    "{\"method\":\"item/agentMessage/delta\",\"params\":{\"threadId\":\"test-thread\",\"turnId\":\"test-turn\",\"itemId\":\"message\",\"delta\":\"Finished ✓\"}}",
                );
                emit(
                    "{\"method\":\"item/completed\",\"params\":{\"threadId\":\"test-thread\",\"turnId\":\"test-turn\",\"item\":{\"id\":\"message\",\"type\":\"agentMessage\",\"text\":\"Finished ✓\"}}}",
                );
                complete("completed");
            } else {
                complete("interrupted");
            }
        }
    }
}
