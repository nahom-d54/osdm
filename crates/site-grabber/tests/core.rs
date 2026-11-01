use site_grabber::*;
use std::collections::BTreeSet;

fn config() -> Config {
    Config {
        seed: "https://example.test/docs/".into(),
        path_prefix: "/docs/".into(),
        extensions: BTreeSet::from(["pdf".into(), "zip".into()]),
        excluded_paths: vec!["/docs/private".into()],
        limits: Limits::default(),
    }
}
fn response(i: &FetchIntent, status: u16, body: &str) -> FetchedResponse {
    FetchedResponse {
        status,
        final_url: i.url.clone(),
        content_type: "text/html; charset=utf-8".into(),
        body: body.as_bytes().to_vec(),
        received_bytes: body.len() as u64,
        location: None,
        content_length: None,
    }
}
fn ready_with(mut c: Config, robots: &str) -> Project {
    c.seed = c.seed.to_string();
    let mut p = Project::new(c).unwrap();
    p.confirm();
    let i = p.next_fetch().unwrap().unwrap();
    p.accept(response(&i, 200, robots)).unwrap();
    p
}
fn ready() -> Project {
    ready_with(config(), "User-agent: *\nDisallow:\n")
}
fn page(p: &mut Project, html: &str) {
    let i = p.next_fetch().unwrap().unwrap();
    p.accept(response(&i, 200, html)).unwrap();
}
fn snapshot(p: &Project) -> Project {
    Project::restore(&serde_json::to_string(p).unwrap()).unwrap()
}

fn metadata(p: &mut Project, url: &str, size: Option<u64>) {
    p.request_metadata(url).unwrap();
    let i = p.next_fetch().unwrap().unwrap();
    assert!(i.kind == FetchKind::ResourceMetadata);
    let mut r = response(&i, 200, "");
    r.content_length = size;
    p.accept(r).unwrap();
}

#[test]
fn explicit_confirmation_and_inflight_pause_resume() {
    let mut p = Project::new(config()).unwrap();
    assert_eq!(p.next_fetch(), Err(Error::NotConfirmed));
    p.confirm();
    let robots = p.next_fetch().unwrap().unwrap();
    assert_eq!(robots.url, "https://example.test/robots.txt");
    p.pause();
    assert_eq!(p.next_fetch(), Err(Error::Paused));
    let mut p = snapshot(&p);
    p.resume();
    assert_eq!(p.next_fetch().unwrap().unwrap(), robots);
    p.accept(response(&robots, 404, "missing")).unwrap();
    page(&mut p, "<a href='one'>one</a><a href='two'>two</a>");
    let first = p.next_fetch().unwrap().unwrap();
    p.pause();
    let mut p = snapshot(&p);
    p.resume();
    assert_eq!(p.next_fetch().unwrap().unwrap(), first);
    p.accept(response(&first, 200, "")).unwrap();
    assert!(p.next_fetch().unwrap().unwrap().url.ends_with("/two"));
}

#[test]
fn canonical_dedup_fragments_queries_and_auth_boundaries() {
    let mut p = ready();
    page(
        &mut p,
        "<a href='./a#one'>A</a><a href='a#two'>B</a>
      <a href='a?sig=one'>C</a><a href='a?sig=two'>D</a>
      <a href='https://user:secret@example.test/docs/auth'>X</a>
      <a href='http://example.test/docs/downgrade'>X</a>
      <a href='https://else.test/docs/cross'>X</a><a href='/outside'>X</a>
      <a href='javascript:alert(1)'>X</a><a href='private/data'>X</a>
      <a href='%70rivate/data'>X</a><form action='submit'></form>",
    );
    let mut urls = Vec::new();
    while let Some(i) = p.next_fetch().unwrap() {
        urls.push(i.url.clone());
        p.accept(response(&i, 200, "")).unwrap();
    }
    assert_eq!(
        urls,
        [
            "https://example.test/docs/a",
            "https://example.test/docs/a?sig=one",
            "https://example.test/docs/a?sig=two"
        ]
    );
}

#[test]
fn malformed_html_resources_are_reviewed_not_fetched_and_names_are_safe() {
    let mut p = ready();
    page(
        &mut p,
        "<a href='../../CON.pdf'>bad<a href='CON.pdf?sig=one#x'>ok<a href='CON.pdf?sig=two'>two<img src='a.zip'><a href='no.exe'>no<script>location='evil'</script>",
    );
    assert!(p.next_fetch().unwrap().is_none());
    let rs: Vec<_> = p.resources().cloned().collect();
    assert_eq!(rs.len(), 3);
    let names: BTreeSet<_> = rs.iter().map(|r| r.filename.clone()).collect();
    assert_eq!(names.len(), 3);
    assert!(
        names
            .iter()
            .all(|n| n.starts_with("resource-") && !n.contains(['/', '\\', ':']))
    );
    assert!(rs.iter().all(|r| r.selected_bytes.is_none()));
    assert_eq!(p.select(&rs[0].url, None).err(), Some(Error::UnknownLength));
    assert_eq!(
        p.select(&rs[0].url, Some(10)).err(),
        Some(Error::UnknownLength)
    );
    metadata(&mut p, &rs[0].url, Some(10));
    assert!(p.select(&rs[0].url, Some(10)).is_ok());
    assert_eq!(snapshot(&p).reserved_bytes(), 10);
}

#[test]
fn budgets_cover_robots_html_and_selection_and_unknown_length_fails() {
    let mut c = config();
    c.limits.bytes = 200;
    let robots = "User-agent: *\nDisallow:\n";
    let mut p = ready_with(c, robots);
    let html = "<a href='a.pdf'>file</a><a href='b.pdf'>other</a>";
    page(&mut p, html);
    let used = robots.len() as u64 + html.len() as u64;
    assert_eq!(p.fetched_bytes(), used);
    let url = p.resources().next().unwrap().url.clone();
    let other = p.resources().nth(1).unwrap().url.clone();
    assert_eq!(p.select(&url, None).err(), Some(Error::UnknownLength));
    metadata(&mut p, &url, Some(200 - used));
    metadata(&mut p, &other, Some(1));
    assert_eq!(
        p.select(&url, Some(201 - used)).err(),
        Some(Error::MetadataMismatch)
    );
    p.select(&url, Some(200 - used)).unwrap();
    assert_eq!(p.select(&other, Some(1)).err(), Some(Error::Budget));
    assert_eq!(p.reserved_bytes(), 200 - used);
    assert!(snapshot(&p).next_fetch().unwrap().is_none());
}

#[test]
fn selection_cannot_consume_issued_request_allowance() {
    let mut p = ready();
    page(&mut p, "<a href='a.pdf'>file</a><a href='next'>next</a>");
    let url = p.resources().next().unwrap().url.clone();
    metadata(&mut p, &url, Some(Limits::default().bytes));
    p.next_fetch().unwrap();
    assert_eq!(
        p.select(&url, Some(Limits::default().bytes)).err(),
        Some(Error::Budget)
    );
}

#[test]
fn page_file_depth_limits_and_cycles_terminate() {
    let mut c = config();
    c.limits.pages = 2;
    c.limits.files = 1;
    c.limits.depth = 1;
    let mut p = ready_with(c, "");
    page(
        &mut p,
        "<a href='a'>a</a><a href='b'>b</a><a href='1.pdf'>one</a><a href='2.pdf'>two</a>",
    );
    page(&mut p, "<a href='./'>loop</a><a href='deep'>deep</a>");
    assert!(p.next_fetch().unwrap().is_none());
    assert_eq!(p.resources().count(), 1);
    snapshot(&p);
}

#[test]
fn robots_union_groups_wildcards_and_allow_never_override_disallow() {
    let mut p = ready_with(
        config(),
        "User-agent: other\nDisallow: /docs/other\nUser-agent: *\nDisallow: /docs/no\nAllow: /docs/no\nUser-agent: OSDM\nDisallow: /docs/*secret$\n",
    );
    page(
        &mut p,
        "<a href='no'>no</a><a href='mysecret'>no</a><a href='other'>yes</a>",
    );
    let i = p.next_fetch().unwrap().unwrap();
    assert!(i.url.ends_with("/other"));
    p.accept(response(&i, 200, "")).unwrap();
    assert!(p.next_fetch().unwrap().is_none());
}

#[test]
fn robots_unavailable_and_redirect_fail_closed() {
    for status in [301, 401, 403, 429, 500] {
        let mut p = Project::new(config()).unwrap();
        p.confirm();
        let i = p.next_fetch().unwrap().unwrap();
        let mut r = response(&i, status, "");
        r.location = Some("https://evil.test/robots.txt".into());
        p.accept(r).unwrap();
        assert!(p.is_stopped());
        assert!(p.next_fetch().unwrap().is_none());
        snapshot(&p);
    }
}

#[test]
fn redirects_are_validated_before_fetch_and_loops_end() {
    for target in [
        "https://evil.test/docs/a",
        "https://user:pass@example.test/docs/a",
        "/outside",
        "/docs/private/a",
        "/docs/%2e%2e/outside",
    ] {
        let mut p = ready();
        let i = p.next_fetch().unwrap().unwrap();
        let mut r = response(&i, 302, "");
        r.location = Some(target.into());
        p.accept(r).unwrap();
        assert!(p.next_fetch().unwrap().is_none(), "{target}");
    }
    let mut p = ready();
    let i = p.next_fetch().unwrap().unwrap();
    let mut r = response(&i, 302, "");
    r.location = Some("next".into());
    p.accept(r).unwrap();
    let i = p.next_fetch().unwrap().unwrap();
    let mut r = response(&i, 302, "");
    r.location = Some("./".into());
    p.accept(r).unwrap();
    assert!(p.next_fetch().unwrap().is_none());
}

#[test]
fn engine_auto_redirect_or_oversized_response_rejected() {
    let mut p = ready();
    let i = p.next_fetch().unwrap().unwrap();
    let mut r = response(&i, 200, "");
    r.final_url = "https://evil.test/docs/".into();
    assert_eq!(p.accept(r), Err(Error::InvalidResponse));
    assert!(p.is_stopped());
    let mut p = ready();
    let i = p.next_fetch().unwrap().unwrap();
    let mut r = response(&i, 200, "");
    r.received_bytes = i.max_bytes + 1;
    assert_eq!(p.accept(r), Err(Error::InvalidResponse));
    assert!(p.is_stopped());
}

#[test]
fn debug_does_not_leak_urls_and_restore_validates() {
    let mut p = ready();
    let i = p.next_fetch().unwrap().unwrap();
    assert!(!format!("{i:?}").contains("example.test"));
    let mut state = serde_json::to_value(&p).unwrap();
    state["in_flight"]["url"] = "https://evil.test/".into();
    assert_eq!(
        Project::restore(&state.to_string()).err(),
        Some(Error::InvalidState)
    );
    let mut state = serde_json::to_value(&p).unwrap();
    state["version"] = 2.into();
    assert_eq!(
        Project::restore(&state.to_string()).err(),
        Some(Error::InvalidState)
    );
}

#[test]
fn bounded_links_and_body_caps() {
    let mut p = ready();
    let i = p.next_fetch().unwrap().unwrap();
    assert_eq!(i.max_bytes, 512 * 1024);
    let html = (0..3000)
        .map(|n| format!("<a href='{n}.pdf'>x</a>"))
        .collect::<String>();
    p.accept(response(&i, 200, &html)).unwrap();
    assert_eq!(p.resources().count(), 100);
}

#[test]
fn resource_redirect_validation_retains_signed_query_identity() {
    let p = ready_with(config(), "User-agent: OSDM\nDisallow: /docs/blocked\n");
    assert_eq!(
        p.validate_target("https://example.test/docs/a.pdf?sig=b%2Fa&x=2#fragment")
            .unwrap(),
        "https://example.test/docs/a.pdf?sig=b%2Fa&x=2"
    );
    for target in [
        "https://evil.test/docs/a.pdf",
        "https://example.test/docs/blocked.pdf",
        "https://example.test/docs/private/a.pdf",
        "https://user:secret@example.test/docs/a.pdf",
        "https://example.test/other/a.pdf",
    ] {
        assert_eq!(p.validate_target(target), Err(Error::OutOfScope));
    }
}

#[test]
fn tampered_filename_and_reservation_rejected_on_reload() {
    let mut p = ready();
    page(&mut p, "<a href='a.pdf'>a</a>");
    let key = p.resources().next().unwrap().url.clone();
    let mut state = serde_json::to_value(&p).unwrap();
    state["resources"][&key]["filename"] = "../CON.pdf".into();
    assert_eq!(
        Project::restore(&state.to_string()).err(),
        Some(Error::InvalidState)
    );
    let mut state = serde_json::to_value(&p).unwrap();
    state["reserved_bytes"] = 100.into();
    assert_eq!(
        Project::restore(&state.to_string()).err(),
        Some(Error::InvalidState)
    );
}

#[test]
fn encoded_robots_rules_deny_conservatively_and_invalid_utf8_stops() {
    let mut p = ready_with(config(), "User-agent: *\nDisallow: /docs/%70rivate\n");
    assert!(p.next_fetch().unwrap().is_none());
    let mut p = Project::new(config()).unwrap();
    p.confirm();
    let i = p.next_fetch().unwrap().unwrap();
    let mut r = response(&i, 200, "");
    r.body = vec![255];
    r.received_bytes = 1;
    p.accept(r).unwrap();
    assert!(p.is_stopped());
    snapshot(&p);
}

#[test]
fn transport_budget_violation_remains_persistable_and_stopped() {
    let mut c = config();
    c.limits.bytes = 2;
    let mut p = Project::new(c).unwrap();
    p.confirm();
    let i = p.next_fetch().unwrap().unwrap();
    assert_eq!(i.max_bytes, 2);
    assert_eq!(
        p.accept(response(&i, 200, "bad")),
        Err(Error::InvalidResponse)
    );
    let mut p = snapshot(&p);
    assert_eq!(p.fetched_bytes(), 3);
    assert!(p.is_stopped());
    assert!(p.next_fetch().unwrap().is_none());
}
