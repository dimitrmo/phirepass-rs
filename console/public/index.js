import init, {
    version,
    ConsoleTerminal
} from '/pkg/debug/phirepass-console.js';

async function setup() {
    await init(); // Load WebAssembly module
}

document.addEventListener("DOMContentLoaded", (event) => {
    setup().then(_ => {
        console.log('>> setup ready', version());
        connect();
    });
});

function connect() {
    const terminal = new ConsoleTerminal(
        // "wss://phirepassf3cceocf-server.functions.fnc.pl-waw.scw.cloud/api/web/ws"
        "ws://localhost:8080/api/web/ws",
    );

    terminal.on_connection_open(() => {
        terminal.start_heartbeat();
        terminal.open_ssh_tunnel("01KCF42JXC7EQQM259GWY1AVQ3");
    });

    terminal.on_connection_message((msg) => {
        console.log(">> on connection message", msg);
    })

    terminal.on_connection_error((error) => {
        console.log('>> on connection error', error);
    });

    terminal.on_connection_close((error) => {
        console.log('>> on connection close', error);
    })

    terminal.on_protocol_message((msg) => {
        console.log(">> on protocol message", msg);
    });

    terminal.connect();
}
