use settings;
use connection;
use api::VcxStateType;
use messages::*;
use messages::message_type::MessageTypes;
use utils::{httpclient, error};


#[derive(Debug)]
pub struct SendMessageBuilder {
    mtype: RemoteMessageType,
    to_did: String,
    to_vk: String,
    agent_did: String,
    agent_vk: String,
    payload: Vec<u8>,
    ref_msg_id: Option<String>,
    status_code: MessageStatusCode,
    uid: Option<String>,
    title: Option<String>,
    detail: Option<String>,
}

impl SendMessageBuilder {
    pub fn create() -> SendMessageBuilder {
        trace!("SendMessage::create_message >>>");

        SendMessageBuilder {
            mtype: RemoteMessageType::Other(String::new()),
            to_did: String::new(),
            to_vk: String::new(),
            agent_did: String::new(),
            agent_vk: String::new(),
            payload: Vec::new(),
            ref_msg_id: None,
            status_code: MessageStatusCode::Created,
            uid: None,
            title: None,
            detail: None,
        }
    }

    pub fn msg_type(&mut self, msg: &RemoteMessageType) -> Result<&mut Self, u32> {
        //Todo: validate msg??
        self.mtype = msg.clone();
        Ok(self)
    }

    pub fn uid(&mut self, uid: Option<&str>) -> Result<&mut Self, u32> {
        //Todo: validate msg_uid??
        self.uid = uid.map(String::from);
        Ok(self)
    }

    pub fn status_code(&mut self, code: &MessageStatusCode) -> Result<&mut Self, u32> {
        //Todo: validate that it can be parsed to number??
        self.status_code = code.clone();
        Ok(self)
    }

    pub fn edge_agent_payload(&mut self, my_vk: &str, their_vk: &str, data: &str, payload_type: PayloadKinds) -> Result<&mut Self, u32> {
        //todo: is this a json value, String??
        self.payload = encrypted_payload(my_vk, their_vk, data, payload_type)?;
        Ok(self)
    }

    pub fn ref_msg_id(&mut self, id: &str) -> Result<&mut Self, u32> {
        self.ref_msg_id = Some(String::from(id));
        Ok(self)
    }

    pub fn set_title(&mut self, title: &str) -> Result<&mut Self, u32> {
        self.title = Some(title.to_string());
        Ok(self)
    }

    pub fn set_detail(&mut self, detail: &str) -> Result<&mut Self, u32> {
        self.detail = Some(detail.to_string());
        Ok(self)
    }

    pub fn send_secure(&mut self) -> Result<SendResponse, u32> {
        trace!("SendMessage::send >>>");

        if settings::test_agency_mode_enabled() {
            return self.parse_response(::utils::constants::SEND_MESSAGE_RESPONSE.to_vec());
        }

        let data = self.prepare_request()?;

        let response = httpclient::post_u8(&data).or(Err(error::POST_MSG_FAILURE.code_num))?;

        let result = self.parse_response(response)?;

        Ok(result)
    }

    fn parse_response(&self, response: Vec<u8>) -> Result<SendResponse, u32> {
        let mut response = parse_response_from_agency(&response)?;

        let index = match settings::get_protocol_type() {
            // TODO: THINK better
            settings::ProtocolTypes::V1 => {
                if response.len() <= 1 {
                    return Err(error::INVALID_HTTP_RESPONSE.code_num);
                }
                1
            },
            settings::ProtocolTypes::V2 => 0
        };

        match response.remove(index) {
            A2AMessage::Version1(A2AMessageV1::MessageSent(res)) =>
                Ok(SendResponse { uid: res.uid, uids: res.uids }),
            A2AMessage::Version2(A2AMessageV2::SendRemoteMessageResponse(res)) =>
                Ok(SendResponse { uid: Some(res.uid.clone()), uids: if res.sent { vec![res.uid] } else { vec![] } }),
            _ => return Err(error::INVALID_HTTP_RESPONSE.code_num)
        }
    }
}

//Todo: Every GeneralMessage extension, duplicates code
impl GeneralMessage for SendMessageBuilder {
    type Msg = SendMessageBuilder;

    fn set_agent_did(&mut self, did: String) { self.agent_did = did; }
    fn set_agent_vk(&mut self, vk: String) { self.agent_vk = vk; }
    fn set_to_did(&mut self, to_did: String) { self.to_did = to_did; }
    fn set_to_vk(&mut self, to_vk: String) { self.to_vk = to_vk; }

    fn prepare_request(&mut self) -> Result<Vec<u8>, u32> {
        let messages =
            match settings::get_protocol_type() {
                settings::ProtocolTypes::V1 => {
                    let create = CreateMessage {
                        msg_type: MessageTypes::build_v1(A2AMessageKinds::CreateMessage),
                        mtype: self.mtype.clone(),
                        reply_to_msg_id: self.ref_msg_id.clone(),
                        send_msg: true,
                        uid: self.uid.clone()
                    };
                    let detail = GeneralMessageDetail {
                        msg_type: MessageTypes::build_v1(A2AMessageKinds::MessageDetail),
                        msg: self.payload.clone(),
                        title: self.title.clone(),
                        detail: self.detail.clone()
                    };
                    vec![A2AMessage::Version1(A2AMessageV1::CreateMessage(create)),
                         A2AMessage::Version1(A2AMessageV1::MessageDetail(MessageDetail::General(detail)))]
                }
                settings::ProtocolTypes::V2 => {
                    let message = SendRemoteMessage {
                        msg_type: MessageTypes::build_v2(A2AMessageKinds::SendRemoteMessage),
                        mtype: self.mtype.clone(),
                        reply_to_msg_id: self.ref_msg_id.clone(),
                        send_msg: true,
                        uid: self.uid.clone(),
                        msg: self.payload.clone(),
                        title: self.title.clone(),
                        detail: self.detail.clone(),
                    };
                    vec![A2AMessage::Version2(A2AMessageV2::SendRemoteMessage(message))]
                }
            };

        prepare_message_for_agent(messages, &self.to_vk, &self.agent_did, &self.agent_vk)
    }
}

#[derive(Debug, PartialEq)]
pub struct SendResponse {
    uid: Option<String>,
    uids: Vec<String>,
}

impl SendResponse {
    pub fn get_msg_uid(&self) -> Result<String, u32> {
        self.uids
            .get(0)
            .map(|uid| uid.to_string())
            .ok_or(error::INVALID_JSON.code_num)
    }
}

pub fn send_generic_message(connection_handle: u32, msg: &str, msg_type: &str, msg_title: &str) -> Result<String, u32> {
    if connection::get_state(connection_handle) != VcxStateType::VcxStateAccepted as u32 {
        return Err(error::NOT_READY.code_num);
    }

    let agent_did = connection::get_agent_did(connection_handle).or(Err(error::INVALID_CONNECTION_HANDLE.code_num))?;
    let agent_vk = connection::get_agent_verkey(connection_handle).or(Err(error::INVALID_CONNECTION_HANDLE.code_num))?;
    let did = connection::get_pw_did(connection_handle).or(Err(error::INVALID_CONNECTION_HANDLE.code_num))?;
    let vk = connection::get_pw_verkey(connection_handle).or(Err(error::INVALID_CONNECTION_HANDLE.code_num))?;
    let remote_vk = connection::get_their_pw_verkey(connection_handle).or(Err(error::INVALID_CONNECTION_HANDLE.code_num))?;

    let response =
        send_message()
            .to(&did)?
            .to_vk(&vk)?
            .msg_type(&RemoteMessageType::Other(msg_type.to_string()))?
            .edge_agent_payload(&vk, &remote_vk, &msg, PayloadKinds::Other(msg_type.to_string()))?
            .agent_did(&agent_did)?
            .agent_vk(&agent_vk)?
            .set_title(&msg_title)?
            .set_detail(&msg_title)?
            .status_code(&MessageStatusCode::Accepted)?
            .send_secure()
            .map_err(|err| {
                warn!("could not send message: {}", err);
                err
            })?;

    let msg_uid = response.get_msg_uid()?;
    return Ok(msg_uid);
}

// TODO: Refactor Error
// this will become a CommonError, because multiple types (Connection/Issuer Credential) use this function
// Possibly this function moves out of this file.
// On second thought, this should stick as a ConnectionError.
pub fn encrypted_payload(my_vk: &str, their_vk: &str, data: &str, msg_type: PayloadKinds) -> Result<Vec<u8>, u32> {
    let payload = ::messages::Payload {
        type_: PayloadTypes::build(msg_type, "json"),
        msg: data.to_string(),
    };

    let bytes = rmp_serde::to_vec_named(&payload)
        .map_err(|err| {
            error!("could not encode create_keys msg: {}", err);
            error::INVALID_MSGPACK.code_num
        })?;

    trace!("Sending payload: {:?}", bytes);
    ::utils::libindy::crypto::prep_msg(&my_vk, &their_vk, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use utils::constants::SEND_MESSAGE_RESPONSE;

    #[test]
    fn test_msgpack() {
        settings::set_defaults();
        settings::set_config_value(settings::CONFIG_ENABLE_TEST_MODE, "true");

        let mut message = SendMessageBuilder {
            mtype: RemoteMessageType::CredOffer,
            to_did: "8XFh8yBzrpJQmNyZzgoTqB".to_string(),
            to_vk: "EkVTa7SCJ5SntpYyX7CSb2pcBhiVGT9kWSagA8a9T69A".to_string(),
            agent_did: "8XFh8yBzrpJQmNyZzgoTqB".to_string(),
            agent_vk: "EkVTa7SCJ5SntpYyX7CSb2pcBhiVGT9kWSagA8a9T69A".to_string(),
            payload: vec![1, 2, 3, 4, 5, 6, 7, 8],
            ref_msg_id: Some("123".to_string()),
            status_code: MessageStatusCode::Created,
            uid: Some("123".to_string()),
            title: Some("this is the title".to_string()),
            detail: Some("this is the detail".to_string()),
        };

        /* just check that it doesn't panic */
        let packed = message.prepare_request().unwrap();
    }

    #[test]
    fn test_parse_send_message_response() {
        init!("true");
        let result = SendMessageBuilder::create().parse_response(SEND_MESSAGE_RESPONSE.to_vec()).unwrap();
        let expected = SendResponse {
            uid: None,
            uids: vec!["ntc2ytb".to_string()],
        };
        assert_eq!(expected, result);
    }

    #[test]
    fn test_parse_send_message_bad_response() {
        init!("true");
        let result = SendMessageBuilder::create().parse_response(::utils::constants::UPDATE_PROFILE_RESPONSE.to_vec());
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_msg_uid() {
        let test_val = "devin";
        let response = SendResponse {
            uid: None,
            uids: vec![test_val.to_string()],
        };

        let uid = response.get_msg_uid().unwrap();
        assert_eq!(test_val, uid);

        let test_val = "devin";
        let response = SendResponse {
            uid: None,
            uids: vec![],
        };

        let uid = response.get_msg_uid().unwrap_err();
        assert_eq!(error::INVALID_JSON.code_num, uid);
    }

    #[cfg(feature = "agency")]
    #[cfg(feature = "pool_tests")]
    #[test]
    fn test_send_generic_message() {
        init!("agency");
        let institution_did = settings::get_config_value(settings::CONFIG_INSTITUTION_DID).unwrap();
        let (faber, alice) = ::connection::tests::create_connected_connections();

        match send_generic_message(alice, "this is the message", "type", "title") {
            Ok(x) => println!("message id: {}", x),
            Err(x) => panic!("paniced! {}", x),
        };
        ::utils::devsetup::tests::set_consumer();
        let all_messages = get_message::download_messages(None, None, None).unwrap();
        println!("{}", serde_json::to_string(&all_messages).unwrap());
        teardown!("agency");
    }

    #[test]
    fn test_send_generic_message_fails_with_invalid_connection() {
        init!("true");
        let handle = ::connection::tests::build_test_connection();

        match send_generic_message(handle, "this is the message", "type", "title") {
            Ok(x) => panic!("test shoudl fail: {}", x),
            Err(x) => assert_eq!(x, error::NOT_READY.code_num),
        };
    }
}
