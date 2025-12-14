include!(concat!(env!("OUT_DIR"), "/version.rs"));

use gloo_timers::callback::Interval;
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::prelude::*;
use web_sys::{BinaryType, CloseEvent, ErrorEvent, MessageEvent, WebSocket};

#[derive(Default)]
struct ConsoleTerminalState {
    socket: Option<WebSocket>,
    heartbeat: Option<Interval>,
    selected_node: Option<String>,
    // pending_username: Option<String>,
    ssh_connected: bool,
    intentional_close: bool,
}

#[derive(Default)]
struct ConsoleTerminalCallbacks {
    on_open: Option<Closure<dyn FnMut()>>,
    on_message: Option<Closure<dyn FnMut(MessageEvent)>>,
    on_close: Option<Closure<dyn FnMut(CloseEvent)>>,
    on_error: Option<Closure<dyn FnMut(ErrorEvent)>>,
}

#[wasm_bindgen]
pub struct ConsoleTerminal {
    state: Rc<RefCell<ConsoleTerminalState>>,
    callbacks: Rc<RefCell<ConsoleTerminalCallbacks>>,
}

#[wasm_bindgen]
impl ConsoleTerminal {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            state: Rc::new(RefCell::new(ConsoleTerminalState::default())),
            callbacks: Rc::new(RefCell::new(ConsoleTerminalCallbacks::default())),
        }
    }

    fn cleanup(&self, intentional: bool) {
        //
    }

    pub fn connect(&self, url: String, node_id: String) {
        self.cleanup(true);

        {
            let mut state = self.state.borrow_mut();
            state.selected_node = Some(node_id);
            state.ssh_connected = false;
            state.intentional_close = false;
        }

        let socket = match WebSocket::new(&url) {
            Ok(ws) => ws,
            Err(err) => {
                // emit_log(&self.state, &format!("WebSocket init error: {err:?}"));
                return;
            }
        };

        socket.set_binary_type(BinaryType::Arraybuffer);

        {
            let mut state = self.state.borrow_mut();
            state.heartbeat = None;
            state.socket = Some(socket);
        }

        let on_open_state = self.state.clone();
        let onopen = Closure::wrap(Box::new(move || {
            start_heartbeat(&on_open_state);
        }) as Box<dyn FnMut()>);

        let mut callbacks = self.callbacks.borrow_mut();
        callbacks.on_open = Some(onopen);
    }
}

fn start_heartbeat(state: &Rc<RefCell<ConsoleTerminalState>>) {
    let state_handle = state.clone();

    let interval = Interval::new(15_000, move || {
        send_heartbeat(&state_handle);
    });

    state.borrow_mut().heartbeat = Some(interval);
}

#[derive(Serialize)]
struct HeartbeatMessage {
    #[serde(rename = "type")]
    msg_type: String,
}

impl Default for HeartbeatMessage {
    fn default() -> Self {
        HeartbeatMessage {
            msg_type: "Heartbeat".to_string(),
        }
    }
}

fn send_heartbeat(state: &Rc<RefCell<ConsoleTerminalState>>) {
    if let Ok(raw) = serde_json::to_vec(&HeartbeatMessage::default()) {
        send_control(state, raw);
    }
}

#[derive(Serialize, Deserialize)]
pub enum Protocol {
    Control = 0,
    SSH = 1,
}

fn encode_frame(protocol: u8, payload: &[u8]) -> Vec<u8> {
    let mut buffer = Vec::with_capacity(5 + payload.len());
    buffer.push(protocol);
    buffer.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    buffer.extend_from_slice(payload);
    buffer
}

fn send_control(state: &Rc<RefCell<ConsoleTerminalState>>, message: Vec<u8>) {
    let frame = encode_frame(Protocol::Control as u8, &message);

    let ready = {
        let inner = state.borrow();
        match inner.socket.as_ref() {
            Some(ws) => ws.ready_state(),
            None => {
                // emit_log(state, "Cannot send control message: socket closed");
                return;
            }
        }
    };

    if ready != WebSocket::OPEN {
        // emit_log(state, "Cannot send control message: socket not open");
        return;
    }

    if let Some(socket) = state.borrow().socket.as_ref() {
        if let Err(err) = socket.send_with_u8_array(&frame) {
            // emit_log(state, &format!("Failed to send control frame: {err:?}"));
        }
    }
}

#[wasm_bindgen]
pub fn version() -> String {
    VERSION.to_string()
}

