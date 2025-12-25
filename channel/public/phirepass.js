import init, {
    Protocol,
    ErrorType,
    Channel as PhirepassChannel,
} from "/pkg/debug/phirepass-channel.js";

async function setup() {
    await init(); // Load WebAssembly module
}

const statusEl = document.getElementById("status");
const logEl = document.getElementById("log");
const connectBtn = document.getElementById("connect");
const terminalHost = document.getElementById("terminal");
const nodesEl = document.getElementById("nodes");
const refreshBtn = document.getElementById("refresh-nodes");
const fullscreenBtn = document.getElementById("fullscreen");

const wsScheme = window.location.protocol === "https:" ? "wss" : "ws";
// const wsEndpoint = `${wsScheme}://${window.location.hostname}:8080`;
// const httpEndpoint = `${window.location.protocol}//${window.location.hostname}:8080`;
const wsEndpoint = `wss://special-space-lamp-x4995657p6wf69x6-8080.app.github.dev`;
const httpEndpoint = `https://special-space-lamp-x4995657p6wf69x6-8080.app.github.dev`;

let term, fitAddon;
let socket;
let nodes = [];
let selectedNodeId = null;
let isIntentionallyClosed = false;
let isSshConnected = false;

let credentialMode = null; // "username" | "password"
let usernameBuffer = "";
let passwordBuffer = "";
let sessionUsername = "";

const log = (text) => {
    const line = document.createElement("div");
    line.className = "log-line";
    line.textContent = `[${new Date().toLocaleTimeString()}] ${text}`;
    logEl.appendChild(line);
    logEl.scrollTop = logEl.scrollHeight;
};

const setStatus = (text, variant = "info") => {
    statusEl.textContent = text;
    const colors = {
        info: "rgba(59, 130, 246, 0.12)",
        ok: "rgba(34, 197, 94, 0.18)",
        warn: "rgba(234, 179, 8, 0.16)",
        error: "rgba(239, 68, 68, 0.18)",
    };
    statusEl.style.background = colors[variant] || colors.info;
};

const formatNumber = (value, digits = 1) =>
    Number.isFinite(value) ? value.toFixed(digits) : "n/a";

const formatBytes = (bytes) => {
    if (!Number.isFinite(bytes)) return "n/a";
    const units = ["B", "KiB", "MiB", "GiB", "TiB"];
    let size = bytes;
    let unit = units.shift();
    while (units.length && size >= 1024) {
        size /= 1024;
        unit = units.shift();
    }
    return `${size.toFixed(1)} ${unit}`;
};

const fetchNodes = async () => {
    try {
        const res = await fetch(`${httpEndpoint}/api/nodes`);
        if (!res.ok) {
            log(`Failed to fetch nodes`);
            console.warn(res.statusText, res);
            return;
        }
        nodes = await res.json();
        renderNodes(nodes);
    } catch (err) {
        log(`Failed to fetch nodes: ${err.message}`);
    }
};

const renderNodes = (list) => {
    nodesEl.innerHTML = "";
    if (!list.length) {
        const empty = document.createElement("div");
        empty.style.color = "#94a3b8";
        empty.textContent = "No nodes connected.";
        nodesEl.appendChild(empty);
        return;
    }

    list.forEach((node) => {
        const card = document.createElement("div");
        card.className = "node-card";
        card.dataset.nodeId = node.id;

        const name = document.createElement("div");
        name.className = "node-name";
        name.textContent = node.id;
        card.appendChild(name);

        const meta = document.createElement("div");
        meta.className = "node-meta";
        const stats = node.stats || {};
        meta.innerHTML = [
            `ip: ${node.ip}`,
            `uptime: ${formatNumber(node.connected_for_secs / 60, 1)} min`,
            `last hb: ${formatNumber(node.since_last_heartbeat_secs, 1)}s`,
            `cpu: ${formatNumber(stats.host_cpu, 1)}%`,
            `host_mem: ${formatBytes(stats.host_mem_used_bytes)} / ${formatBytes(
                stats.host_mem_total_bytes
            )}`,
        ]
            .map((line) => `<div>${line}</div>`)
            .join("");
        card.appendChild(meta);

        card.addEventListener("click", () => {
            // Check if there's an active websocket connection
            if (socket && socket.is_open()) {
                // Already connected, warn user
                const confirmed = confirm(
                    `You are currently connected to a node. Do you want to disconnect and switch to ${node.id}?`
                );
                if (!confirmed) {
                    // User canceled - do nothing
                    return;
                }
            }

            selectedNodeId = node.id;
            Array.from(nodesEl.children).forEach((el) =>
                el.classList.toggle("selected", el.dataset.nodeId === node.id)
            );
            log(`Selected node ${node.id}`);
            socket = connect();
        });

        nodesEl.appendChild(card);
    });
};

const cleanup = () => {
    if (socket) {
        isIntentionallyClosed = true;
        socket.disconnect();
        socket = null;
    }

    resetCredentialCapture();
    sessionUsername = "";
    isSshConnected = false;
    fitAddon.fit();
};

const resetCredentialCapture = () => {
    credentialMode = null;
    usernameBuffer = "";
    passwordBuffer = "";
};

const promptForUsername = () => {
    resetCredentialCapture();
    sessionUsername = "";
    term.reset();
    term.write("Enter username: ");
    credentialMode = "username";
    setStatus("Username required", "warn");
};

const promptForPassword = (shouldReset = false) => {
    if (shouldReset) {
        term.reset();
    } else {
        term.writeln("");
    }
    passwordBuffer = "";
    credentialMode = "password";
    term.write("Enter password: ");
    setStatus("Enter password", "warn");
};

const submitUsername = () => {
    const username = usernameBuffer.trim();
    if (!username.length) {
        log("Username is required to start SSH session");
        term.writeln("");
        term.write("Enter username: ");
        usernameBuffer = "";
        return;
    }

    sessionUsername = username;
    promptForPassword(true);
};

const cancelCredentialEntry = () => {
    resetCredentialCapture();
    log("Credential entry cancelled");
    setStatus("Idle", "warn");
    cleanup();
};

const handleUsernameKeystroke = (data) => {
    if (data === "\r" || data === "\n") {
        term.write("\r\n");
        submitUsername();
        return;
    }

    if (data === "\u0003") {
        term.write("^C\r\n");
        cancelCredentialEntry();
        return;
    }

    if (data === "\u007f") {
        if (usernameBuffer.length) {
            usernameBuffer = usernameBuffer.slice(0, -1);
            term.write("\b \b");
        }
        return;
    }

    if (data >= " " && data <= "~") {
        usernameBuffer += data;
        term.write(data);
    }
};

const submitPassword = () => {
    const password = passwordBuffer;
    resetCredentialCapture();

    if (!password.length) {
        log("Password is required to start SSH session");
        promptForPassword();
        return;
    }

    if (!sessionUsername) {
        log("Username is required before submitting password");
        promptForUsername();
        return;
    }

    setStatus("Authenticating...", "info");
    log(
        `Credentials submitted for user, attempting SSH connection...`
    );

    if (socket_healthy()) {
        socket.open_sftp_tunnel(selectedNodeId, sessionUsername, password);
        // socket.open_ssh_tunnel(selectedNodeId, sessionUsername, password);
        // socket.send_terminal_resize(selectedNodeId, term.cols, term.rows);
    }
};

const handlePasswordKeystroke = (data) => {
    // Enter submits the captured password.
    if (data === "\r" || data === "\n") {
        term.write("\r\n");
        submitPassword();
        return;
    }

    // Ctrl+C cancels capture and disconnects.
    if (data === "\u0003") {
        term.write("^C\r\n");
        cancelCredentialEntry();
        return;
    }

    // Handle backspace - no visual feedback to hide password length.
    if (data === "\u007f") {
        if (passwordBuffer.length) {
            passwordBuffer = passwordBuffer.slice(0, -1);
        }
        return;
    }

    // Accept only printable characters - silently capture without visual feedback.
    if (data >= " " && data <= "~") {
        passwordBuffer += data;
    }
};

function socket_healthy() {
    if (socket) {
        if (socket.is_open()) {
            return true;
        }
    }

    return false;
}

function connect() {
    if (!selectedNodeId) {
        log("Select a node before connecting");
        return;
    }

    // Close any active channel before opening a new one.
    cleanup();

    term.reset();
    fitAddon.fit();
    setStatus("Connecting...");

    const channel = new PhirepassChannel(`${wsEndpoint}/api/web/ws`);

    channel.on_connection_open(() => {
        channel.start_heartbeat();
        channel.open_sftp_tunnel(selectedNodeId);
        log("WebSocket connected");
        setStatus("Connecting to node...", "info");
    });

    channel.on_connection_message((event) => {
        // console.log(">> on connection message", event);
    });

    channel.on_connection_error((event) => {
        setStatus("Error", "error");
        log(`Socket error: ${event.message ?? "unknown error"}`);
    });

    channel.on_connection_close((event) => {
        // console.log(">> on connection close", event);
        if (!isIntentionallyClosed) {
            setStatus("Disconnected", "warn");
            const reason = event.reason || `code ${event.code}`;
            log(`Socket closed (${reason})`);
            term.reset();
            cleanup();
        } else {
            log("WebSocket connection closed");
        }
        isIntentionallyClosed = false;
    });

    channel.on_protocol_message((protocol, msg) => {
        if (protocol === Protocol.SSH) {
            if (!isSshConnected) {
                isSshConnected = true;
                const target = selectedNodeId || "selected node";
                log(`SSH login successful on ${target}`);
                setStatus("Connected", "ok");
                term.reset();
            }
            term.write(new Uint8Array(msg));
            return;
        }

        if (protocol === Protocol.SFTP) {
            //
        }

        if (msg && !msg.type) {
            console.error("Wrong control message format", msg);
            return;
        }

        switch (msg.type) {
            case "TunnelOpened":
                log(`Tunnel opened (id=${msg.session_id})`);
                setStatus(`Tunnel opened (id=${msg.session_id})`, "ok");
                socket.send_sftp_tunnel_data(selectedNodeId, "Hello moto");
                break;
            case "TunnelClosed":
                log(`Tunnel closed by remote host`);
                setStatus("Tunnel closed", "warn");
                term.reset();
                cleanup();
                break;
            case "Error":
                switch (msg.kind) {
                    case ErrorType.RequiresUsernamePassword:
                        term.reset();
                        setStatus("Credentials required", "warn");
                        log("SSH username and password are required.");
                        promptForUsername();
                        break;
                    case ErrorType.RequiresPassword:
                        term.reset();
                        setStatus("Password required", "warn");
                        log("SSH password is required.");
                        if (!sessionUsername) {
                            promptForUsername();
                        } else {
                            promptForPassword();
                        }
                        break;
                    case ErrorType.Generic:
                    default:
                        term.reset();
                        const message =
                            msg.message ||
                            "An unknown error occurred during SSH authentication.";
                        setStatus("Auth error", "error");
                        log(message);
                        sessionUsername = "";
                        isSshConnected = false;
                        promptForUsername();
                }
                break;
            default:
                term.reset();
                const message =
                    msg.message ||
                    "SSH authentication failed. Please try again.";
                setStatus("Auth failed", "error");
                log(message);
                sessionUsername = "";
                isSshConnected = false;
                promptForUsername();
        }
    });

    channel.connect();

    return channel;
}

function setup_terminal() {
    const term = new Terminal({
        convertEol: true,
        cursorBlink: true,
        fontFamily:
            '"Berkeley Mono", "Fira Code", "SFMono-Regular", Menlo, monospace',
        fontSize: 14,
        allowProposedApi: true, // needed for bracketed paste
        rightClickSelectsWord: false,
        bellStyle: "sound",
        disableStdin: false,
        windowsMode: false,
        logLevel: "info",
        theme: {
            background: "#0b1021",
            foreground: "#e2e8f0",
            cursor: "#67e8f9",
        },
    });
    const fitAddon = new FitAddon.FitAddon();
    term.loadAddon(fitAddon);
    term.open(terminalHost);
    fitAddon.fit();
    term.focus();
    term.pasteMode = "bracketed"; // enable bracketed paste sequences
    return [term, fitAddon];
}

document.addEventListener("DOMContentLoaded", () => {
    connectBtn.addEventListener("click", connect);
    refreshBtn.addEventListener("click", fetchNodes);

    fullscreenBtn.addEventListener("click", () => {
        const container = document.documentElement;
        if (!document.fullscreenElement) {
            container.requestFullscreen().catch((err) => {
                log(`Failed to enter fullscreen: ${err.message}`);
            });
        } else {
            document.exitFullscreen().catch((err) => {
                log(`Failed to exit fullscreen: ${err.message}`);
            });
        }
    });

    [term, fitAddon] = setup_terminal();

    term.onData((data) => {
        if (credentialMode === "username") {
            handleUsernameKeystroke(data);
            return;
        }

        if (credentialMode === "password") {
            handlePasswordKeystroke(data);
            return;
        }

        if (socket && socket.is_open() && !!selectedNodeId) {
            socket.send_tunnel_data(selectedNodeId, data);
        }
    });

    term.onResize(({ cols, rows }) => {
        if (socket && socket.is_open() && !!selectedNodeId) {
            socket.send_terminal_resize(selectedNodeId, cols, rows);
        }
    });

    const resizeObserver = new ResizeObserver(() => {
        fitAddon.fit();
        if (socket && socket.is_open() && !!selectedNodeId) {
            socket.send_terminal_resize(selectedNodeId, term.cols, term.rows);
        }
    });

    resizeObserver.observe(terminalHost);

    terminalHost.addEventListener("click", () => {
        term.focus();
    });

    setup().then(fetchNodes);
});
