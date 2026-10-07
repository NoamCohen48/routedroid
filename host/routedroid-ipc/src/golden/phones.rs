use super::*;

fn pixel() -> Phone {
    Phone {
        serial: "R58M".into(),
        name: Some("pixel".into()),
        auto: true,
        lan_if: Some("eno1".into()),
        ..Phone::default()
    }
}

#[test]
fn remembering_and_forgetting() {
    pinned(
        ask(1, Request::Remember(pixel())),
        r#"{"id":1,"type":"remember","serial":"R58M","name":"pixel","auto":true,"lan_if":"eno1"}"#,
    );
    pinned(
        ask(
            2,
            Request::Forget {
                phone: "pixel".into(),
            },
        ),
        r#"{"id":2,"type":"forget","phone":"pixel"}"#,
    );
    pinned(ask(3, Request::Phones), r#"{"id":3,"type":"phones"}"#);
    pinned(
        answer(Response::Phones {
            phones: vec![pixel()],
        }),
        concat!(
            r#"{"msg":"response","id":7,"type":"phones","phones":[{"serial":"R58M","#,
            r#""name":"pixel","auto":true,"lan_if":"eno1"}]}"#
        ),
    );
    let phone: Phone = serde_json::from_str(r#"{"serial":"R58M"}"#).unwrap();
    assert!(!phone.auto && phone.name.is_none());
}

#[test]
fn a_named_phone_reads_as_name_and_serial() {
    assert_eq!(pixel().label(), "pixel (R58M)");
    assert_eq!(label("R58M", None), "R58M");
}
