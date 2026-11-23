// Minimal Discord IPC server for verifying MonoCode's Rich Presence locally.
// Usage: node scripts/mock-discord-ipc.mjs [port 0..9]
// Pair with MONOCODE_DISCORD_IPC_PORT=<port> npm run tauri dev.
import fs from "node:fs";
import net from "node:net";
import path from "node:path";

const port = Number(process.argv[2] ?? 0);
if (!Number.isInteger(port) || port < 0 || port > 9) {
  console.error("Port must be a number from 0 to 9");
  process.exit(2);
}

const directory = process.env.XDG_RUNTIME_DIR || "/tmp";
const socketPath = path.join(directory, `discord-ipc-${port}`);
if (fs.existsSync(socketPath)) {
  console.error(`${socketPath} already exists — stop that client or pick another port`);
  process.exit(1);
}

function frame(opcode, payload) {
  const body = Buffer.from(JSON.stringify(payload));
  const header = Buffer.alloc(8);
  header.writeUInt32LE(opcode, 0);
  header.writeUInt32LE(body.length, 4);
  return Buffer.concat([header, body]);
}

const ready = frame(1, { cmd: "DISPATCH", evt: "READY", data: {} });

const clients = new Set();

const server = net.createServer((socket) => {
  console.log("client connected");
  clients.add(socket);
  socket.on("close", () => clients.delete(socket));
  let buffer = Buffer.alloc(0);
  socket.on("data", (chunk) => {
    buffer = Buffer.concat([buffer, chunk]);
    while (buffer.length >= 8) {
      const opcode = buffer.readUInt32LE(0);
      const length = buffer.readUInt32LE(4);
      if (length > 1024 * 1024) {
        console.error(`frame too large: ${length}`);
        socket.destroy();
        return;
      }
      if (buffer.length < 8 + length) break;
      const body = buffer.subarray(8, 8 + length).toString("utf8");
      buffer = buffer.subarray(8 + length);
      console.log(`[opcode ${opcode}] ${body}`);
      if (opcode === 0) socket.write(ready);
    }
  });
  socket.on("close", () => console.log("client disconnected"));
  socket.on("error", (error) => console.error(`socket: ${error.message}`));
});

server.listen(socketPath, () => {
  console.log(`Mock Discord IPC listening on ${socketPath}`);
});

let shuttingDown = false;
function shutdown() {
  if (shuttingDown) return;
  shuttingDown = true;
  // server.close() waits for open connections, so end them first. With no
  // clients this destroys nothing and the close callback still runs.
  for (const socket of clients) socket.destroy();
  clients.clear();
  server.close(() => {
    try {
      fs.unlinkSync(socketPath);
    } catch {
      // Already gone.
    }
    process.exit(0);
  });
}
process.on("SIGINT", shutdown);
process.on("SIGTERM", shutdown);
