#![cfg(feature = "file")]
//! Adversary pass 1, wave 2026-10-20-w10: a `file:~/` reference refused under a home that is not
//! absolute names no part of the reference or the home.

use llm_core::ErrorCode;
use llm_models::routes::Routes;

fn document(routes: &str) -> String {
    format!(
        "format: llm.routes/1
endpoints:
  p: {{base_url: https://p.example.invalid/v1, protocol: messages, auth: {{kind: anonymous}}}}
routes:
{routes}"
    )
}

/// A `file:~/` reference under a home that is not absolute is refused naming the endpoint and no
/// part of the reference or the home, as the refusal without any home already is in `routes.rs`.
#[cfg(feature = "file")]
#[test]
fn adversary_w10_a_relative_home_refusal_names_no_part_of_the_file_reference() {
    let document = document("  r: p/m\n").replace(
        "auth: {kind: anonymous}",
        "auth: {kind: bearer, secret: \"file:~/llm-routes-dir/llm-routes-token\"}",
    );
    let routes = Routes::parse(&document).unwrap();
    for home in ["relative-home", "/llm-routes-home/../elsewhere"] {
        let resolvers = routes
            .resolvers(&llm_models::routes::SecretSources::new().with_home(home))
            .unwrap();
        let error = routes
            .port(
                "r",
                llm_http::HttpClient::new(llm_http::Limits::default()).unwrap(),
                &resolvers,
            )
            .err()
            .unwrap();
        assert_eq!(error.code, ErrorCode::Unauthorized, "{home}");
        assert!(error.message.contains("`p`"), "{home}: {}", error.message);
        for part in ["llm-routes-dir", "llm-routes-token", home] {
            assert!(
                !error.message.contains(part),
                "{home}: {} echoes `{part}`",
                error.message
            );
        }
    }
}
