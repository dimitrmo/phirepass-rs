import init, {
    version,
    ConsoleTerminal
} from '/pkg/debug/phirepass-console.js';

async function setup() {
    await init(); // Load WebAssembly module
}

document.addEventListener("DOMContentLoaded", (event) => {
    setup().then(_ => console.log('>> setup ready', version()));
});
