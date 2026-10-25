use osdm_browser_bridge::{forward, read_frame, write_frame};
use serde_json::json;

#[tokio::main]
async fn main() {
    if std::env::args().any(|arg| arg == "--native-test-info") {
        let enabled = osdm_browser_bridge::native_test_directory().is_ok_and(|dir| dir.is_some());
        let _ = write_frame(
            &mut std::io::stdout().lock(),
            &json!({"native_test":enabled}),
        );
        return;
    }
    // stdout is exclusively native-message framing. Never print input or URLs.
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap();
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    while let Ok(Some(message)) = read_frame(&mut input) {
        let response = forward(&client, &message)
            .await
            .unwrap_or_else(|| json!({"ok":false,"error":"disconnected"}));
        if write_frame(&mut output, &response).is_err() {
            break;
        }
    }
}
