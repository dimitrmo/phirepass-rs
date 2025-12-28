include!(concat!(env!("OUT_DIR"), "/version.rs"));

use gloo_timers::callback::Interval;
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::prelude::*;
use web_sys::js_sys::Function;
use web_sys::js_sys::Uint8Array;
use web_sys::{BinaryType, CloseEvent, ErrorEvent, MessageEvent, WebSocket};

macro_rules! console_warn {
    ($($t:tt)*) => (warn(&format_args!($($t)*).to_string()))
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn warn(s: &str);
}

#[derive(Default)]
struct ChannelState {
    socket: Option<WebSocket>,
    heartbeat: Option<Interval>,
}

#[derive(Default)]
struct ChannelClosures {
    on_open: Option<Closure<dyn FnMut()>>,
    on_message: Option<Closure<dyn FnMut(MessageEvent)>>,
    on_close: Option<Closure<dyn FnMut(CloseEvent)>>,
    on_error: Option<Closure<dyn FnMut(ErrorEvent)>>,
}

#[derive(Default)]
struct ChannelCallbacks {
    on_connection_open: Option<Function>,
    on_connection_error: Option<Function>,
    on_connection_close: Option<Function>,
    on_connection_message: Option<Function>,
    on_protocol_message: Option<Function>,
}

#[wasm_bindgen]
pub struct Channel {
    endpoint: String,
    state: Rc<RefCell<ChannelState>>,
    closures: Rc<RefCell<ChannelClosures>>,
    callbacks: Rc<RefCell<ChannelCallbacks>>,
}

impl Clone for Channel {
    fn clone(&self) -> Self {
        Self {
            endpoint: self.endpoint.clone(),
            state: self.state.clone(),
            closures: self.closures.clone(),
            callbacks: self.callbacks.clone(),
        }
    }
}

#[wasm_bindgen]
impl Channel {
    #[wasm_bindgen(constructor)]
    pub fn new(endpoint: String) -> Self {
        Self {
            endpoint,
            state: Rc::new(RefCell::new(ChannelState::default())),
            closures: Rc::new(RefCell::new(ChannelClosures::default())),
            callbacks: Rc::new(RefCell::new(ChannelCallbacks::default())),
        }
    }

    pub fn connect(&self) {
        let socket = match WebSocket::new(&self.endpoint) {
            Ok(ws) => ws,
            Err(err) => {
                console_warn!("{}", &format!("WebSocket init error: {err:?}"));
                return;
            }
        };

        socket.set_binary_type(BinaryType::Arraybuffer);

        {
            let mut state = self.state.borrow_mut();
            state.heartbeat = None;
            state.socket = Some(socket);
        }

        // on open

        let connected_callback = self.callbacks.borrow().on_connection_open.clone();
        let onopen = Closure::wrap(Box::new(move || {
            if let Some(cb) = connected_callback.as_ref() {
                let _ = cb.call0(&JsValue::NULL);
            }
        }) as Box<dyn FnMut()>);

        if let Some(ws) = self.state.borrow_mut().socket.as_ref() {
            ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
        }

        // on error

        let connection_error_cb = self.callbacks.borrow().on_connection_error.clone();
        let onerror = Closure::wrap(Box::new(move |event: ErrorEvent| {
            if let Some(cb) = connection_error_cb.as_ref() {
                let _ = cb.call1(&JsValue::NULL, &JsValue::from(event));
            }
        }) as Box<dyn FnMut(ErrorEvent)>);

        if let Some(ws) = self.state.borrow_mut().socket.as_ref() {
            ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        }

        // on message
        let protocol_message_cb = self.callbacks.borrow().on_protocol_message.clone();
        let connection_message_cb = self.callbacks.borrow().on_connection_message.clone();
        let onmessage = Closure::wrap(Box::new(move |event: MessageEvent| {
            if let Some(cb) = connection_message_cb.as_ref() {
                let _ = cb.call1(&JsValue::NULL, &JsValue::from(&event));
            }
            if let Some(cb) = protocol_message_cb.as_ref() {
                handle_message(&cb, &event);
            }
        }) as Box<dyn FnMut(MessageEvent)>);

        if let Some(ws) = self.state.borrow_mut().socket.as_ref() {
            ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        }

        // on close
        let connection_close_cb = self.callbacks.borrow().on_connection_close.clone();
        let onclose = Closure::wrap(Box::new(move |event: CloseEvent| {
            if let Some(cb) = connection_close_cb.as_ref() {
                let _ = cb.call1(&JsValue::NULL, &JsValue::from(event));
            }
        }) as Box<dyn FnMut(CloseEvent)>);

        if let Some(ws) = self.state.borrow_mut().socket.as_ref() {
            ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
        }

        let mut closures = self.closures.borrow_mut();
        closures.on_open = Some(onopen);
        closures.on_error = Some(onerror);
        closures.on_message = Some(onmessage);
        closures.on_close = Some(onclose);
    }

    pub fn on_connection_open(&self, cb: Option<Function>) {
        self.callbacks.borrow_mut().on_connection_open = cb;
    }

    pub fn on_connection_error(&self, cb: Option<Function>) {
        self.callbacks.borrow_mut().on_connection_error = cb;
    }

    pub fn on_connection_message(&self, cb: Option<Function>) {
        self.callbacks.borrow_mut().on_connection_message = cb;
    }

    pub fn on_connection_close(&self, cb: Option<Function>) {
        self.callbacks.borrow_mut().on_connection_close = cb;
    }

    pub fn on_protocol_message(&self, cb: Option<Function>) {
        self.callbacks.borrow_mut().on_protocol_message = cb;
    }

    pub fn stop_heartbeat(&self) {
        if let Some(interval) = self.state.borrow_mut().heartbeat.take() {
            interval.cancel();
        }

        self.state.borrow_mut().heartbeat = None;
    }

    pub fn start_heartbeat(&self, mut interval_as_millis: u32) {
        self.stop_heartbeat();

        if interval_as_millis == 0 {
            interval_as_millis = 15_000;
        }

        if let Ok(raw) = serde_json::to_vec(&HeartbeatMessage::new()) {
            self.send_raw(Protocol::Control as u8, raw);
        }

        let Ok(raw) = serde_json::to_vec(&HeartbeatMessage::new()) else {
            return;
        };

        let channel = self.clone();
        let interval = Interval::new(interval_as_millis, move || {
            channel.send_raw(Protocol::Control as u8, raw.clone());
        });

        self.state.borrow_mut().heartbeat = Some(interval);
    }

    pub fn open_ssh_tunnel(
        &self,
        node_id: String,
        username: Option<String>,
        password: Option<String>,
    ) {
        if !self.is_open() {
            return;
        }

        if let Ok(raw) = serde_json::to_vec(&OpenTunnelMessage::new(
            Protocol::SSH as u8,
            node_id,
            username,
            password,
        )) {
            self.send_raw(Protocol::Control as u8, raw);
        }
    }

    pub fn send_terminal_resize(&self, node_id: String, cols: u32, rows: u32) {
        if !self.is_open() {
            return;
        }

        if let Ok(raw) = serde_json::to_vec(&ResizeTerminal::new(node_id, cols, rows)) {
            self.send_raw(Protocol::Control as u8, raw);
        }
    }

    pub fn send_tunnel_data(&self, node_id: String, data: String) {
        if !self.is_open() {
            return;
        }

        if let Ok(raw) = serde_json::to_vec(&TunnelData::new(
            Protocol::SSH as u8,
            node_id,
            data.into_bytes(),
        )) {
            // console_warn!("tunnel data sent");
            // Tunnel data must travel inside a control frame; the server will
            // unwrap and forward the payload to the SSH tunnel.
            self.send_raw(Protocol::Control as u8, raw);
        }
    }

    pub fn is_open(&self) -> bool {
        if let Some(socket) = self.state.borrow().socket.as_ref() {
            socket.ready_state() == WebSocket::OPEN
        } else {
            false
        }
    }

    fn send_raw(&self, protocol: u8, message: Vec<u8>) {
        let frame = encode_frame(protocol, &message);

        if !self.is_open() {
            console_warn!("Cannot send raw message: socket not open");
            return;
        }

        if let Some(socket) = self.state.borrow().socket.as_ref() {
            if let Err(err) = socket.send_with_u8_array(&frame) {
                console_warn!("{}", format!("Failed to send raw frame: {err:?}"));
            }
        }
    }

    pub fn disconnect(&self) {
        self.stop_heartbeat();
        if let Some(socket) = self.state.borrow_mut().socket.take() {
            let _ = socket.close();
        }
    }
}

#[derive(Serialize)]
struct HeartbeatMessage {
    #[serde(rename = "type")]
    msg_type: String,
}

impl HeartbeatMessage {
    fn new() -> Self {
        HeartbeatMessage {
            msg_type: "Heartbeat".to_string(),
        }
    }
}

#[derive(Serialize)]
struct OpenTunnelMessage {
    #[serde(rename = "type")]
    msg_type: String,
    protocol: u8,
    target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    password: Option<String>,
}

impl OpenTunnelMessage {
    fn new(
        protocol: u8,
        target: String,
        username: Option<String>,
        password: Option<String>,
    ) -> Self {
        OpenTunnelMessage {
            msg_type: "OpenTunnel".to_string(),
            protocol,
            target,
            username,
            password,
        }
    }
}

#[derive(Serialize)]
struct ResizeTerminal {
    #[serde(rename = "type")]
    msg_type: String,
    target: String,
    cols: u32,
    rows: u32,
}

impl ResizeTerminal {
    fn new(target: String, cols: u32, rows: u32) -> Self {
        ResizeTerminal {
            msg_type: "Resize".to_string(),
            target,
            cols,
            rows,
        }
    }
}

#[derive(Serialize)]
struct TunnelData {
    #[serde(rename = "type")]
    msg_type: String,
    protocol: u8,
    target: String,
    data: Vec<u8>,
}

impl TunnelData {
    fn new(protocol: u8, target: String, data: Vec<u8>) -> Self {
        TunnelData {
            msg_type: "TunnelData".to_string(),
            protocol,
            target,
            data,
        }
    }
}

#[repr(u8)]
#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorType {
    Generic = 0,
    RequiresPassword = 100,
    RequiresUsernamePassword = 110,
}

#[wasm_bindgen]
#[derive(Serialize, Deserialize)]
pub enum Protocol {
    Control = 0,
    SSH = 1,
}

impl From<u8> for Protocol {
    fn from(val: u8) -> Self {
        match val {
            1 => Protocol::SSH,
            _ => Protocol::Control,
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
        console_warn!("Frame is too short");
        return None;
    }

    let protocol = bytes[0];
    let length = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as usize;
    if bytes.len() < 5 + length {
        console_warn!("Invalid frame format");
        return None;
    }

    Some((protocol, bytes[5..5 + length].to_vec()))
}

fn handle_message(cb: &Function, event: &MessageEvent) {
    if let Some(text) = event.data().as_string() {
        console_warn!("received text from: {}", text);
        return;
    }

    let buffer: web_sys::js_sys::ArrayBuffer = match event.data().dyn_into() {
        Ok(buf) => buf,
        Err(err) => {
            console_warn!("{:?}", err);
            return;
        }
    };

    let view = Uint8Array::new(&buffer);
    let mut data = vec![0u8; view.length() as usize];
    view.copy_to(&mut data);

    let (protocol, payload) = match decode_frame(&data) {
        Some(parts) => parts,
        None => {
            console_warn!("received invalid frame");
            return;
        }
    };

    match Protocol::from(protocol) {
        Protocol::Control => {
            handle_control_frame(cb, &payload);
        }
        Protocol::SSH => {
            handle_ssh_frame(cb, &payload);
        }
    }
}

fn handle_control_frame(cb: &Function, payload: &[u8]) {
    let message = match String::from_utf8(payload.to_vec()) {
        Ok(msg) => msg,
        Err(err) => {
            console_warn!("{}", err);
            return;
        }
    };

    let control: serde_json::Value = match serde_json::from_str(&message) {
        Ok(msg) => msg,
        Err(err) => {
            console_warn!("{}", err);
            return;
        }
    };

    let serializer = serde_wasm_bindgen::Serializer::new()
        .serialize_maps_as_objects(true);

    let js_value = match control.serialize(&serializer) {
        Ok(msg) => msg,
        Err(err) => {
            console_warn!("{}", err);
            return;
        }
    };

    let _ = cb.call2(&JsValue::NULL, &JsValue::from(Protocol::Control), &js_value);
}

fn handle_ssh_frame(cb: &Function, payload: &[u8]) {
    let data = Uint8Array::from(payload);
    let _ = cb.call2(&JsValue::NULL, &JsValue::from(Protocol::SSH), &data.into());
}

#[wasm_bindgen]
pub fn version() -> String {
    VERSION.to_string()
}
