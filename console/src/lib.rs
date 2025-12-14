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
struct ConsoleTerminalState {
    socket: Option<WebSocket>,
    heartbeat: Option<Interval>,
}

#[derive(Default)]
struct ConsoleTerminalClosures {
    on_open: Option<Closure<dyn FnMut()>>,
    on_message: Option<Closure<dyn FnMut(MessageEvent)>>,
    on_close: Option<Closure<dyn FnMut(CloseEvent)>>,
    on_error: Option<Closure<dyn FnMut(ErrorEvent)>>,
}

#[derive(Default)]
struct ConsoleTerminalCallbacks {
    on_connection_open: Option<Function>,
    on_connection_error: Option<Function>,
    on_connection_close: Option<Function>,
    on_connection_message: Option<Function>,
    on_protocol_message: Option<Function>,
}

#[wasm_bindgen]
pub struct ConsoleTerminal {
    endpoint: String,
    state: Rc<RefCell<ConsoleTerminalState>>,
    closures: Rc<RefCell<ConsoleTerminalClosures>>,
    callbacks: Rc<RefCell<ConsoleTerminalCallbacks>>,
}

#[wasm_bindgen]
impl ConsoleTerminal {
    #[wasm_bindgen(constructor)]
    pub fn new(endpoint: String) -> Self {
        Self {
            endpoint,
            state: Rc::new(RefCell::new(ConsoleTerminalState::default())),
            closures: Rc::new(RefCell::new(ConsoleTerminalClosures::default())),
            callbacks: Rc::new(RefCell::new(ConsoleTerminalCallbacks::default())),
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

        let state_handle = self.state.clone();

        if let Ok(raw) = serde_json::to_vec(&HeartbeatMessage::new()) {
            send_raw(&state_handle, Protocol::Control as u8, raw);
        }

        let interval = Interval::new(interval_as_millis, move || {
            if let Ok(raw) = serde_json::to_vec(&HeartbeatMessage::new()) {
                send_raw(&state_handle, Protocol::Control as u8, raw);
            }
        });

        self.state.borrow_mut().heartbeat = Some(interval);
    }

    pub fn open_ssh_tunnel(
        &self,
        target: String,
        username: Option<String>,
        password: Option<String>,
    ) {
        if let Ok(raw) = serde_json::to_vec(&OpenTunnelMessage::new(
            Protocol::SSH as u8,
            target,
            username,
            password,
        )) {
            send_raw(&self.state, Protocol::Control as u8, raw);
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

fn send_raw(state: &Rc<RefCell<ConsoleTerminalState>>, protocol: u8, message: Vec<u8>) {
    let frame = encode_frame(protocol, &message);

    let ready = {
        let inner = state.borrow();
        match inner.socket.as_ref() {
            Some(ws) => ws.ready_state(),
            None => {
                console_warn!("Cannot send control message: socket closed");
                return;
            }
        }
    };

    if ready != WebSocket::OPEN {
        console_warn!("Cannot send control message: socket not open");
        return;
    }

    if let Some(socket) = state.borrow().socket.as_ref() {
        if let Err(err) = socket.send_with_u8_array(&frame) {
            console_warn!("{}", format!("Failed to send control frame: {err:?}"));
        }
    }
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

#[derive(Debug, Serialize, Deserialize)]
pub struct IncomingControl {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub message: Option<String>,
    pub kind: Option<u32>,
}

fn handle_control_frame(cb: &Function, payload: &[u8]) {
    let message = match String::from_utf8(payload.to_vec()) {
        Ok(msg) => msg,
        Err(err) => {
            console_warn!("{}", err);
            return;
        }
    };

    let control: IncomingControl = match serde_json::from_str(&message) {
        Ok(msg) => msg,
        Err(err) => {
            console_warn!("{}", err);
            return;
        }
    };

    let js_value = match serde_wasm_bindgen::to_value(&control) {
        Ok(msg) => msg,
        Err(err) => {
            console_warn!("{}", err);
            return;
        }
    };

    let _ = cb.call1(&JsValue::NULL, &js_value);
}

fn handle_ssh_frame(cb: &Function, payload: &[u8]) {
    let data = Uint8Array::from(payload);
    let _ = cb.call1(&JsValue::NULL, &data.into());
}

#[wasm_bindgen]
pub fn version() -> String {
    VERSION.to_string()
}
