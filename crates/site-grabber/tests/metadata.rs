use site_grabber::*;
use std::collections::BTreeSet;

fn response(i: &FetchIntent, status: u16, body: &str, length: Option<u64>) -> FetchedResponse {
    FetchedResponse {
        status,
        final_url: i.url.clone(),
        content_type: "text/html".into(),
        body: body.as_bytes().to_vec(),
        received_bytes: body.len() as u64,
        location: None,
        content_length: length,
    }
}

fn project() -> Project {
    Project::create(CreateProjectRequest {
        confirmed: true,
        config: ProjectConfig {
            source: "https://example.test/docs/".into(),
            scope: Scope {
                path_prefix: "/docs/".into(),
            },
            filters: Filters {
                extensions: BTreeSet::from(["pdf".into()]),
                excluded_paths: vec![],
            },
            budgets: Budgets::default(),
        },
    })
    .unwrap()
}

fn discovered() -> Project {
    let mut p = project();
    let i = p.next_fetch().unwrap().unwrap();
    p.accept(response(&i, 404, "", None)).unwrap();
    let i = p.next_fetch().unwrap().unwrap();
    p.accept(response(&i, 200, "<a href='a.pdf?sig=one'>file</a>", None))
        .unwrap();
    p
}

#[test]
fn wizard_dto_and_head_resume_are_stable_and_selection_requires_known_metadata() {
    let mut p = discovered();
    let snapshot = p.snapshot();
    let dto = serde_json::to_value(snapshot).unwrap();
    assert_eq!(dto["config"]["source"], "https://example.test/docs/");
    assert_eq!(dto["config"]["scope"]["path_prefix"], "/docs/");
    assert!(dto["resources"][0]["known_bytes"].is_null());
    let url = p.resources().next().unwrap().url.clone();
    assert_eq!(p.select(&url, Some(0)).err(), Some(Error::UnknownLength));
    p.request_metadata(&url).unwrap();
    p.request_metadata(&url).unwrap();
    assert_eq!(p.snapshot().pending_fetches, 1);
    let i = p.next_fetch().unwrap().unwrap();
    assert!(i.kind == FetchKind::ResourceMetadata);
    p.pause();
    let mut p = Project::restore(&serde_json::to_string(&p).unwrap()).unwrap();
    p.resume();
    assert_eq!(p.next_fetch().unwrap().unwrap(), i);
    let before = p.fetched_bytes();
    p.accept(response(&i, 200, "", Some(123))).unwrap();
    assert_eq!(p.fetched_bytes(), before);
    assert_eq!(p.resources().next().unwrap().known_bytes, Some(123));
    assert_eq!(
        p.select(&url, Some(122)).err(),
        Some(Error::MetadataMismatch)
    );
    p.select(&url, Some(123)).unwrap();
    assert_eq!(p.reserved_bytes(), 123);
    p.request_metadata(&url).unwrap();
    assert!(p.next_fetch().unwrap().is_none());
}

#[test]
fn unknown_head_length_and_redirect_never_become_zero_or_followed() {
    for status in [200, 302, 404, 500] {
        let mut p = discovered();
        let url = p.resources().next().unwrap().url.clone();
        p.request_metadata(&url).unwrap();
        let i = p.next_fetch().unwrap().unwrap();
        let mut r = response(&i, status, "", None);
        r.location = Some("https://evil.test/b.pdf".into());
        p.accept(r).unwrap();
        assert!(p.resources().next().unwrap().known_bytes.is_none());
        assert_eq!(p.select(&url, Some(0)).err(), Some(Error::UnknownLength));
        assert!(p.next_fetch().unwrap().is_none());
        Project::restore(&serde_json::to_string(&p).unwrap()).unwrap();
    }
}

#[test]
fn unexpected_head_body_is_charged_but_not_trusted() {
    let mut p = discovered();
    let url = p.resources().next().unwrap().url.clone();
    p.request_metadata(&url).unwrap();
    let i = p.next_fetch().unwrap().unwrap();
    let before = p.fetched_bytes();
    p.accept(response(&i, 200, "unexpected", Some(99))).unwrap();
    assert_eq!(p.fetched_bytes(), before + 10);
    assert!(p.resources().next().unwrap().known_bytes.is_none());
    assert_eq!(p.select(&url, Some(99)).err(), Some(Error::UnknownLength));
}

#[test]
fn wizard_unconfirmed_cannot_probe_or_fetch() {
    let request = CreateProjectRequest {
        config: project().snapshot().config,
        confirmed: false,
    };
    let mut p = Project::create(request).unwrap();
    assert_eq!(p.next_fetch(), Err(Error::NotConfirmed));
    assert_eq!(
        p.request_metadata("https://example.test/docs/a.pdf"),
        Err(Error::NotConfirmed)
    );
    assert!(!p.snapshot().confirmed);
}
