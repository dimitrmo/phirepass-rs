
/*
use std::{cell::RefCell, rc::Rc};

use gloo_timers::callback::Interval;
use js_sys::{Function, JsString, Uint8Array};
use serde::{Deserialize, Serialize};
use wasm_bindgen::{JsCast, prelude::*};
use web_sys::{BinaryType, CloseEvent, ErrorEvent, MessageEvent, WebSocket};

const PROTOCOL_CONTROL: u8 = 0;
const PROTOCOL_SSH: u8 = 1;

const REQUIRES_PASSWORD: u32 = 100;
const REQUIRES_USERNAME_PASSWORD: u32 = 110;

#[wasm_bindgen]
pub enum AuthRequirement {
    UsernamePassword,
    Password,
    Failed,
}

#[derive(Default)]
struct Callbacks {
    on_log: Option<Function>,
    on_status: Option<Function>,
    on_ssh_data: Option<Function>,
    on_auth_required: Option<Function>,
    on_tunnel_closed: Option<Function>,
    on_connected: Option<Function>,
    on_close: Option<Function>,
}

struct EventClosures {
    on_open: Option<Closure<dyn FnMut()>>,
    on_message: Option<Closure<dyn FnMut(MessageEvent)>>,
    on_close: Option<Closure<dyn FnMut(CloseEvent)>>,
    on_error: Option<Closure<dyn FnMut(ErrorEvent)>>,
}

impl Default for EventClosures {
    fn default() -> Self {
        Self {
            on_open: None,
            on_message: None,
            on_close: None,
            on_error: None,
        }
    }
}

#[derive(Default)]
struct InnerState {
    socket: Option<WebSocket>,
    heartbeat: Option<Interval>,
    callbacks: Callbacks,
    closures: EventClosures,
    selected_node: Option<String>,
    pending_username: Option<String>,
    ssh_connected: bool,
    intentional_close: bool,
}

#[wasm_bindgen]
pub struct ConsoleClient {
    state: Rc<RefCell<InnerState>>,
}

#[wasm_bindgen]
impl ConsoleClient {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            state: Rc::new(RefCell::new(InnerState::default())),
        }
    }

    pub fn set_on_log(&self, cb: Option<Function>) {
        self.state.borrow_mut().callbacks.on_log = cb;
    }

    pub fn set_on_status(&self, cb: Option<Function>) {
        self.state.borrow_mut().callbacks.on_status = cb;
    }

    pub fn set_on_ssh_data(&self, cb: Option<Function>) {
        self.state.borrow_mut().callbacks.on_ssh_data = cb;
    }

    pub fn set_on_auth_required(&self, cb: Option<Function>) {
        self.state.borrow_mut().callbacks.on_auth_required = cb;
    }

    pub fn set_on_tunnel_closed(&self, cb: Option<Function>) {
        self.state.borrow_mut().callbacks.on_tunnel_closed = cb;
    }

    pub fn set_on_connected(&self, cb: Option<Function>) {
        self.state.borrow_mut().callbacks.on_connected = cb;
    }

    pub fn set_on_close(&self, cb: Option<Function>) {
        self.state.borrow_mut().callbacks.on_close = cb;
    }

    pub fn connect(&self, url: String, target: String, username: Option<String>) {
        self.cleanup(true);

        {
            let mut state = self.state.borrow_mut();
            state.selected_node = Some(target);
            state.pending_username = username;
            state.ssh_connected = false;
            state.intentional_close = false;
        }

        let socket = match WebSocket::new(&url) {
            Ok(ws) => ws,
            Err(err) => {
                emit_log(&self.state, &format!("WebSocket init error: {err:?}"));
                return;
            }
        };

        socket.set_binary_type(BinaryType::Arraybuffer);
        self.attach_handlers(socket, url);
    }

    pub fn disconnect(&self) {
        self.cleanup(true);
    }

    pub fn provide_credentials(&self, username: Option<String>, password: Option<String>) {
        {
            let mut state = self.state.borrow_mut();
            if username.is_some() {
                state.pending_username = username.clone();
            }
        }
        send_open_tunnel(&self.state, username, password);
    }

    #[wasm_bindgen(js_name = send_terminal_data)]
    pub fn send_terminal_data(&self, data: Vec<u8>) {
        let target = {
            let state = self.state.borrow();
            if state.socket.is_none() {
                emit_log(&self.state, "Cannot send SSH data: socket not connected");
                return;
            }
            match state.selected_node.clone() {
                Some(node) => node,
                None => {
                    emit_log(&self.state, "Cannot send SSH data: no node selected");
                    return;
                }
            }
        };

        let payload = ControlEnvelope {
            msg_type: "TunnelData".to_string(),
            protocol: Some(PROTOCOL_SSH),
            target: Some(target),
            username: None,
            password: None,
            cols: None,
            rows: None,
            data: Some(data),
        };
        send_control(&self.state, &payload);
    }

    #[wasm_bindgen(js_name = send_resize)]
    pub fn send_resize(&self, cols: u32, rows: u32) {
        let target = {
            let state = self.state.borrow();
            match state.selected_node.clone() {
                Some(node) => node,
                None => return,
            }
        };

        let payload = ControlEnvelope {
            msg_type: "Resize".to_string(),
            protocol: None,
            target: Some(target),
            username: None,
            password: None,
            cols: Some(cols),
            rows: Some(rows),
            data: None,
        };
        send_control(&self.state, &payload);
    }
}

impl ConsoleClient {
    fn attach_handlers(&self, socket: WebSocket, url: String) {
        let state = self.state.clone();
        {
            let mut inner = state.borrow_mut();
            inner.heartbeat = None;
            inner.socket = Some(socket);
        }

        let on_open_state = state.clone();
        let onopen = Closure::wrap(Box::new(move || {
            emit_log(&on_open_state, &format!("WebSocket connected to {url}"));
            emit_status(&on_open_state, "Connecting to node...", Some("info"));
            send_heartbeat(&on_open_state);
            start_heartbeat(&on_open_state);
            send_open_tunnel(&on_open_state, None, None);
        }) as Box<dyn FnMut()>);

        let on_message_state = state.clone();
        let onmessage = Closure::wrap(Box::new(move |event: MessageEvent| {
            handle_message(&on_message_state, event);
        }) as Box<dyn FnMut(MessageEvent)>);

        let on_close_state = state.clone();
        let onclose = Closure::wrap(Box::new(move |event: CloseEvent| {
            let intentional = {
                let mut inner = on_close_state.borrow_mut();
                inner.heartbeat = None;
                inner.socket = None;
                inner.ssh_connected = false;
                inner.intentional_close
            };

            let reason = {
                let reason = event.reason();
                if reason.is_empty() {
                    format!("code {}", event.code())
                } else {
                    reason
                }
            };

            emit_close(&on_close_state, reason, intentional);
        }) as Box<dyn FnMut(CloseEvent)>);

        let on_error_state = state.clone();
        let onerror = Closure::wrap(Box::new(move |event: ErrorEvent| {
            emit_status(&on_error_state, "Socket error", Some("error"));
            emit_log(
                &on_error_state,
                &format!("Socket error: {}", event.message()),
            );
        }) as Box<dyn FnMut(ErrorEvent)>);

        if let Some(ws) = state.borrow().socket.as_ref() {
            ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
            ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
            ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
            ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        }

        let mut inner = state.borrow_mut();
        inner.closures.on_open = Some(onopen);
        inner.closures.on_message = Some(onmessage);
        inner.closures.on_close = Some(onclose);
        inner.closures.on_error = Some(onerror);
    }

    fn cleanup(&self, intentional: bool) {
        let mut state = self.state.borrow_mut();
        state.ssh_connected = false;
        state.intentional_close = intentional;
        state.heartbeat = None;
        state.closures = EventClosures::default();

        if let Some(socket) = state.socket.take() {
            let _ = socket.close();
        }
    }
}

#[derive(Serialize)]
struct ControlEnvelope {
    #[serde(rename = "type")]
    msg_type: String,
    protocol: Option<u8>,
    target: Option<String>,
    username: Option<String>,
    password: Option<String>,
    cols: Option<u32>,
    rows: Option<u32>,
    data: Option<Vec<u8>>,
}

#[derive(Deserialize)]
struct IncomingControl {
    #[serde(rename = "type")]
    msg_type: String,
    message: Option<String>,
    kind: Option<u32>,
}

fn send_control(state: &Rc<RefCell<InnerState>>, payload: &ControlEnvelope) {
    let frame = match serde_json::to_vec(payload) {
        Ok(bytes) => encode_frame(PROTOCOL_CONTROL, &bytes),
        Err(err) => {
            emit_log(state, &format!("Failed to encode control payload: {err}"));
            return;
        }
    };

    let ready = {
        let inner = state.borrow();
        match inner.socket.as_ref() {
            Some(ws) => ws.ready_state(),
            None => {
                emit_log(state, "Cannot send control message: socket closed");
                return;
            }
        }
    };

    if ready != WebSocket::OPEN {
        emit_log(state, "Cannot send control message: socket not open");
        return;
    }

    if let Some(socket) = state.borrow().socket.as_ref() {
        if let Err(err) = socket.send_with_u8_array(&frame) {
            emit_log(state, &format!("Failed to send control frame: {err:?}"));
        }
    }
}

fn send_heartbeat(state: &Rc<RefCell<InnerState>>) {
    let envelope = ControlEnvelope {
        msg_type: "Heartbeat".to_string(),
        protocol: None,
        target: None,
        username: None,
        password: None,
        cols: None,
        rows: None,
        data: None,
    };
    send_control(state, &envelope);
}

fn send_open_tunnel(
    state: &Rc<RefCell<InnerState>>,
    username: Option<String>,
    password: Option<String>,
) {
    let target = {
        let inner = state.borrow();
        match inner.selected_node.clone() {
            Some(node) => node,
            None => {
                emit_log(state, "No target selected for tunnel request");
                return;
            }
        }
    };

    let envelope = ControlEnvelope {
        msg_type: "OpenTunnel".to_string(),
        protocol: Some(PROTOCOL_SSH),
        target: Some(target),
        username: username.or_else(|| state.borrow().pending_username.clone()),
        password,
        cols: None,
        rows: None,
        data: None,
    };
    send_control(state, &envelope);
    emit_status(state, "Authenticating...", Some("info"));
}

fn start_heartbeat(state: &Rc<RefCell<InnerState>>) {
    let state_handle = state.clone();
    let interval = Interval::new(15_000, move || {
        send_heartbeat(&state_handle);
    });

    state.borrow_mut().heartbeat = Some(interval);
}

fn handle_message(state: &Rc<RefCell<InnerState>>, event: MessageEvent) {
    if let Some(text) = event.data().as_string() {
        emit_log(
            state,
            &format!("Received text frame ({}) bytes", text.len()),
        );
        return;
    }

    let buffer: js_sys::ArrayBuffer = match event.data().dyn_into() {
        Ok(buf) => buf,
        Err(_) => {
            emit_log(state, "Dropped non-binary frame");
            return;
        }
    };

    let view = Uint8Array::new(&buffer);
    let mut data = vec![0u8; view.length() as usize];
    view.copy_to(&mut data);

    let (protocol, payload) = match decode_frame(&data) {
        Some(parts) => parts,
        None => {
            emit_log(state, "Dropped malformed frame");
            return;
        }
    };

    match protocol {
        PROTOCOL_SSH => {
            let mut inner = state.borrow_mut();
            if !inner.ssh_connected {
                inner.ssh_connected = true;
                drop(inner);
                emit_status(state, "Connected", Some("ok"));
                emit_log(state, "SSH connection established");
                emit_connected(state);
            } else {
                drop(inner);
            }
            emit_ssh_data(state, &payload);
        }
        PROTOCOL_CONTROL => handle_control_frame(state, &payload),
        other => {
            emit_log(state, &format!("Unknown protocol frame received: {other}"));
        }
    }
}

fn handle_control_frame(state: &Rc<RefCell<InnerState>>, payload: &[u8]) {
    let message = match String::from_utf8(payload.to_vec()) {
        Ok(msg) => msg,
        Err(err) => {
            emit_log(state, &format!("Control parse error: {err}"));
            return;
        }
    };

    let control: IncomingControl = match serde_json::from_str(&message) {
        Ok(msg) => msg,
        Err(err) => {
            emit_log(state, &format!("Control parse error: {err}"));
            return;
        }
    };

    match control.msg_type.as_str() {
        "TunnelClosed" => {
            emit_log(state, "SSH tunnel closed by remote host");
            emit_status(state, "Tunnel closed", Some("warn"));
            emit_tunnel_closed(state);
            let mut inner = state.borrow_mut();
            inner.ssh_connected = false;
            inner.heartbeat = None;
            inner.intentional_close = true;
            if let Some(socket) = inner.socket.take() {
                let _ = socket.close();
            }
        }
        "Error" => match control.kind.unwrap_or_default() {
            REQUIRES_USERNAME_PASSWORD => {
                emit_status(state, "Credentials required", Some("warn"));
                emit_log(state, "SSH username and password are required to continue.");
                state.borrow_mut().ssh_connected = false;
                emit_auth_required(state, AuthRequirement::UsernamePassword);
            }
            REQUIRES_PASSWORD => {
                emit_status(state, "Password required", Some("warn"));
                emit_log(state, "SSH password is required to continue.");
                state.borrow_mut().ssh_connected = false;
                emit_auth_required(state, AuthRequirement::Password);
            }
            _ => {
                emit_status(state, "Auth failed", Some("error"));
                emit_log(
                    state,
                    &control
                        .message
                        .unwrap_or_else(|| "SSH authentication failed".into()),
                );
                state.borrow_mut().ssh_connected = false;
                emit_auth_required(state, AuthRequirement::Failed);
            }
        },
        other => {
            emit_log(state, &format!("Control: {other}"));
        }
    }
}

fn encode_frame(protocol: u8, payload: &[u8]) -> Vec<u8> {
    let mut buffer = Vec::with_capacity(5 + payload.len());
    buffer.push(protocol);
    buffer.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    buffer.extend_from_slice(payload);
    buffer
}

fn decode_frame(bytes: &[u8]) -> Option<(u8, Vec<u8>)> {
    if bytes.len() < 5 {
        return None;
    }

    let protocol = bytes[0];
    let length = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as usize;
    if bytes.len() < 5 + length {
        return None;
    }

    Some((protocol, bytes[5..5 + length].to_vec()))
}

fn emit_log(state: &Rc<RefCell<InnerState>>, message: &str) {
    if let Some(cb) = state.borrow().callbacks.on_log.as_ref() {
        let _ = cb.call1(&JsValue::NULL, &JsValue::from(message));
    } else {
        web_sys::console::log_1(&JsValue::from(message));
    }
}

fn emit_status(state: &Rc<RefCell<InnerState>>, text: &str, variant: Option<&str>) {
    if let Some(cb) = state.borrow().callbacks.on_status.as_ref() {
        let variant = variant.unwrap_or("info");
        let _ = cb.call2(
            &JsValue::NULL,
            &JsValue::from(text),
            &JsValue::from(variant),
        );
    }
}

fn emit_ssh_data(state: &Rc<RefCell<InnerState>>, payload: &[u8]) {
    if let Some(cb) = state.borrow().callbacks.on_ssh_data.as_ref() {
        let data = Uint8Array::from(payload);
        let _ = cb.call1(&JsValue::NULL, &data.into());
    }
}

fn emit_auth_required(state: &Rc<RefCell<InnerState>>, requirement: AuthRequirement) {
    if let Some(cb) = state.borrow().callbacks.on_auth_required.as_ref() {
        let _ = cb.call1(&JsValue::NULL, &JsValue::from(requirement as u32));
    }
}

fn emit_tunnel_closed(state: &Rc<RefCell<InnerState>>) {
    if let Some(cb) = state.borrow().callbacks.on_tunnel_closed.as_ref() {
        let _ = cb.call0(&JsValue::NULL);
    }
}

fn emit_connected(state: &Rc<RefCell<InnerState>>) {
    if let Some(cb) = state.borrow().callbacks.on_connected.as_ref() {
        let _ = cb.call0(&JsValue::NULL);
    }
}

fn emit_close(state: &Rc<RefCell<InnerState>>, reason: String, intentional: bool) {
    if let Some(cb) = state.borrow().callbacks.on_close.as_ref() {
        let _ = cb.call2(
            &JsValue::NULL,
            &JsValue::from(JsString::from(reason)),
            &JsValue::from(intentional),
        );
    } else if !intentional {
        emit_log(state, &format!("Socket closed ({reason})"));
    }
}
*/
