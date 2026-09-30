// Server-sent events parser per the WHATWG event-stream format: lines end in CRLF, LF, or CR,
// a blank line dispatches the event, and `data:` lines join with "\n". Chunk boundaries may fall
// anywhere, including between the CR and LF of one line ending.

export type SseParser = {
  /** Feeds a decoded chunk; returns the data payloads of events completed by it. */
  push(chunk: string): string[];
  /** Flushes at end of stream; an unterminated final event is discarded, as the spec requires. */
  end(): string[];
};

export function createSseParser(): SseParser {
  let buf = "";
  let data: string[] = [];
  let dropLeadingLf = false; // previous chunk ended in CR that may pair with this chunk's LF

  const line = (l: string, out: string[]) => {
    if (l === "") {
      if (data.length > 0) out.push(data.join("\n"));
      data = [];
      return;
    }
    if (l.startsWith(":")) return; // comment
    const colon = l.indexOf(":");
    const field = colon < 0 ? l : l.slice(0, colon);
    let value = colon < 0 ? "" : l.slice(colon + 1);
    if (value.startsWith(" ")) value = value.slice(1);
    if (field === "data") data.push(value);
  };

  return {
    push(chunk) {
      const out: string[] = [];
      if (dropLeadingLf && chunk.startsWith("\n")) chunk = chunk.slice(1);
      dropLeadingLf = false;
      buf += chunk;
      let start = 0;
      for (let i = 0; i < buf.length; i++) {
        const c = buf[i];
        if (c !== "\n" && c !== "\r") continue;
        line(buf.slice(start, i), out);
        if (c === "\r") {
          if (i + 1 < buf.length) {
            if (buf[i + 1] === "\n") i++;
          } else dropLeadingLf = true;
        }
        start = i + 1;
      }
      buf = buf.slice(start);
      return out;
    },
    end() {
      buf = "";
      data = [];
      return [];
    },
  };
}
