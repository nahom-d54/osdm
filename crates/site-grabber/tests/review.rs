use site_grabber::*;
use std::collections::BTreeSet;

fn project(html: bool) -> Project {
    Project::create(CreateProjectRequest {
        confirmed: true,
        config: ProjectConfig {
            source: "https://example.test/docs/index.html".into(),
            scope: Scope {
                path_prefix: "/docs/".into(),
            },
            filters: Filters {
                extensions: if html {
                    BTreeSet::from(["pdf".into(), "html".into(), "htm".into()])
                } else {
                    BTreeSet::from(["pdf".into()])
                },
                excluded_paths: vec![],
            },
            budgets: Budgets {
                pages: 10_000,
                files: 10_000,
                bytes: 100 * 1024 * 1024,
                depth: 16,
            },
        },
    })
    .unwrap()
}

fn response(intent: &FetchIntent, body: &str) -> FetchedResponse {
    FetchedResponse {
        status: 200,
        final_url: intent.url.clone(),
        content_type: "text/html".into(),
        body: body.as_bytes().to_vec(),
        received_bytes: body.len() as u64,
        location: None,
        content_length: None,
    }
}

fn roundtrip(project: &Project) -> Project {
    let json = serde_json::to_string(project).unwrap();
    assert!(json.len() < 64 * 1024 * 1024);
    Project::restore(&json).unwrap()
}

fn ready(html: bool) -> Project {
    let mut p = project(html);
    let robots = p.next_fetch().unwrap().unwrap();
    p.accept(response(&robots, "User-agent: *\nDisallow:\n"))
        .unwrap();
    roundtrip(&p)
}

#[test]
fn leading_utf8_bom_does_not_bypass_robots_disallow() {
    for agent in ["*", "OSDM"] {
        let mut p = project(true);
        let robots = p.next_fetch().unwrap().unwrap();
        p.accept(response(
            &robots,
            &format!("\u{feff}User-agent: {agent}\nDisallow: /docs\n"),
        ))
        .unwrap();
        assert!(p.next_fetch().unwrap().is_none());
        assert_eq!(p.resources().count(), 0);
        assert_eq!(
            p.validate_target("https://example.test/docs/index.html"),
            Err(Error::OutOfScope)
        );
        roundtrip(&p);
    }
    let mut p = project(true);
    let robots = p.next_fetch().unwrap().unwrap();
    p.accept(response(
        &robots,
        "\u{feff}\u{feff}User-agent: *\nDisallow: /docs\n",
    ))
    .unwrap();
    assert!(p.next_fetch().unwrap().is_none());
    roundtrip(&p);
}

#[test]
fn explicit_html_filter_keeps_crawling_and_review_and_distinct_pending_kinds() {
    let mut p = ready(true);
    let seed = p.next_fetch().unwrap().unwrap();
    assert!(seed.kind == FetchKind::Page);
    assert!(p.resources().any(|r| r.url == seed.url));
    // A HEAD and GET for the same HTML identity may coexist, even in flight.
    p.request_metadata(&seed.url).unwrap();
    p = roundtrip(&p);
    let mut duplicate = serde_json::to_value(&p).unwrap();
    let head = duplicate["frontier"][0].clone();
    duplicate["frontier"].as_array_mut().unwrap().push(head);
    assert_eq!(
        Project::restore(&duplicate.to_string()).err(),
        Some(Error::InvalidState)
    );
    assert_eq!(p.next_fetch().unwrap().unwrap(), seed);
    p.accept(response(&seed, "<a href='next.htm?q=one#first'>next</a><a href='next.htm?q=one#again'>same</a><a href='next.htm?q=two'>different</a><a href='a.pdf'>pdf</a>")).unwrap();
    p = roundtrip(&p);
    let head = p.next_fetch().unwrap().unwrap();
    assert!(head.kind == FetchKind::ResourceMetadata);
    let mut fetched = response(&head, "");
    fetched.content_length = Some(123);
    p.accept(fetched).unwrap();
    p.select(&seed.url, Some(123)).unwrap();
    assert_eq!(p.resources().count(), 4);
    let first = p.next_fetch().unwrap().unwrap();
    assert!(first.kind == FetchKind::Page);
    assert!(first.url.ends_with("next.htm?q=one"));
    p.accept(response(&first, "<a href='discovered.pdf'>deeper</a>"))
        .unwrap();
    let second = p.next_fetch().unwrap().unwrap();
    assert!(second.url.ends_with("next.htm?q=two"));
    p.accept(response(&second, "")).unwrap();
    assert!(p.next_fetch().unwrap().is_none());
    assert!(p.resources().any(|r| r.url.ends_with("discovered.pdf")));
    let saved = serde_json::to_value(&p).unwrap();
    assert_eq!(saved["pages"], 3);
    assert_eq!(saved["seen"].as_array().unwrap().len(), 5);
    roundtrip(&p);
}

#[test]
fn forged_pdf_metadata_intent_cannot_become_a_page() {
    let mut p = ready(false);
    let seed = p.next_fetch().unwrap().unwrap();
    p.accept(response(&seed, "<a href='a.pdf'>pdf</a>"))
        .unwrap();
    let url = p.resources().next().unwrap().url.clone();
    p.request_metadata(&url).unwrap();
    for in_flight in [false, true] {
        if in_flight {
            p.next_fetch().unwrap();
        }
        let mut saved = serde_json::to_value(&p).unwrap();
        if in_flight {
            saved["in_flight"]["kind"] = "Page".into();
        } else {
            saved["frontier"][0]["kind"] = "Page".into();
        }
        assert_eq!(
            Project::restore(&saved.to_string()).err(),
            Some(Error::InvalidState)
        );
    }
    roundtrip(&p);
}

#[test]
fn legacy_disjoint_page_identities_restore_without_admitting_pdf_pages() {
    let mut p = ready(false);
    let seed = p.next_fetch().unwrap().unwrap();
    p.accept(response(&seed, "<a href='a.pdf'>pdf</a>"))
        .unwrap();
    let mut saved = serde_json::to_value(&p).unwrap();
    saved.as_object_mut().unwrap().remove("page_urls");
    let mut restored = Project::restore(&saved.to_string()).unwrap();
    let url = restored.resources().next().unwrap().url.clone();
    restored.request_metadata(&url).unwrap();
    let mut saved = serde_json::to_value(&restored).unwrap();
    saved.as_object_mut().unwrap().remove("page_urls");
    saved["frontier"][0]["kind"] = "Page".into();
    assert_eq!(
        Project::restore(&saved.to_string()).err(),
        Some(Error::InvalidState)
    );
}

#[test]
fn long_url_discovery_stops_with_budget_and_every_admitted_state_restores() {
    let mut p = ready(false);
    // Public API only: each document is <512 KiB and the body budget allows all
    // 3000 candidates. Distinct long queries stress copies and JSON escaping.
    let padding = "\\".repeat(7900);
    let mut exhausted = false;
    for batch in 0..60 {
        let intent = p.next_fetch().unwrap().unwrap();
        p = roundtrip(&p);
        let mut html = format!("<a href='page-{batch}'>next</a>");
        for index in 0..50 {
            html.push_str(&format!(
                "<a href='file-{}.pdf?q={padding}'>file</a>",
                batch * 50 + index
            ));
        }
        assert!(html.len() < 512 * 1024);
        match p.accept(response(&intent, &html)) {
            Ok(()) => {}
            Err(Error::Budget) => {
                exhausted = true;
            }
            Err(error) => panic!("unexpected error: {error}"),
        }
        p = roundtrip(&p);
        let urls: Vec<_> = p.resources().map(|r| r.url.clone()).collect();
        // Queue every possible HEAD to stress the additional persisted copy.
        for url in urls {
            let result = p.request_metadata(&url);
            if exhausted {
                assert_eq!(result, Err(Error::Budget));
            } else {
                result.unwrap();
            }
        }
        p = roundtrip(&p);
        if exhausted {
            break;
        }
        // Drain metadata so discovery reaches the next small HTML body.
        loop {
            let next = p.next_fetch().unwrap().unwrap();
            if next.kind == FetchKind::Page {
                break;
            }
            assert!(next.kind == FetchKind::ResourceMetadata);
            p.accept(response(&next, "")).unwrap();
        }
    }
    assert!(exhausted);
    assert!(p.is_stopped());
    assert!(p.resources().count() < 3000);
    assert!(p.next_fetch().unwrap().is_none());
    roundtrip(&p);
}
