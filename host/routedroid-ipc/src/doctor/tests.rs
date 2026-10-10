use crate::{Check, CheckStatus, ClientMessage, Request, Response, ServerMessage};

#[test]
fn wire_shape_is_pinned() {
    let ask = ClientMessage {
        id: 9,
        request: Request::Doctor { repair: true },
    };
    let json = r#"{"id":9,"type":"doctor","repair":true}"#;
    assert_eq!(serde_json::to_string(&ask).unwrap(), json);
    assert_eq!(serde_json::from_str::<ClientMessage>(json).unwrap(), ask);
    // `repair` may be left out: a look only.
    let look: ClientMessage = serde_json::from_str(r#"{"id":9,"type":"doctor"}"#).unwrap();
    assert_eq!(look.request, Request::Doctor { repair: false });

    let mut table = Check::new(
        "nft table inet routedroid_phone0",
        CheckStatus::Fail,
        "left over",
    );
    table
        .repair
        .push("delete table inet routedroid_phone0".into());
    let answer = ServerMessage::Response {
        id: 9,
        response: Response::Doctor {
            checks: vec![Check::new("adb", CheckStatus::Ok, "1 device"), table],
            done: vec![],
        },
    };
    let json = concat!(
        r#"{"msg":"response","id":9,"type":"doctor","checks":["#,
        r#"{"name":"adb","status":"ok","detail":"1 device","repair":[]},"#,
        r#"{"name":"nft table inet routedroid_phone0","status":"fail","detail":"left over","#,
        r#""repair":["delete table inet routedroid_phone0"]}],"done":[]}"#
    );
    assert_eq!(serde_json::to_string(&answer).unwrap(), json);
    assert_eq!(serde_json::from_str::<ServerMessage>(json).unwrap(), answer);
}
