export type Block =
  | { type: "heading"; level: number; text: string }
  | { type: "para"; text: string }
  | { type: "list"; items: string[] }
  | { type: "table"; head: string[]; rows: string[][] };

const cells = (line: string) => line.trim().replace(/^\|/, "").replace(/\|$/, "").split("|").map((c) => c.trim());

/** A small, safe reader for the markdown the architect writes: headings, bullets and pipe tables. Everything stays text; nothing is turned into HTML. */
export function parseMarkdown(md: string): Block[] {
  const out: Block[] = [];
  const lines = md.replace(/\r\n/g, "\n").split("\n");
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (!line.trim()) { i++; continue; }
    const h = /^(#{1,6})\s+(.*)$/.exec(line);
    if (h) { out.push({ type: "heading", level: h[1].length, text: h[2].trim() }); i++; continue; }
    if (line.trim().startsWith("|")) {
      // a row can wrap onto the next line, so join lines until the next row or the end of the table
      const rows: string[] = [];
      while (i < lines.length && lines[i].trim() && (lines[i].trim().startsWith("|") || rows.length)) {
        if (lines[i].trim().startsWith("|")) rows.push(lines[i].trim());
        else rows[rows.length - 1] += " " + lines[i].trim();
        i++;
      }
      const parsed = rows.map(cells);
      const body = parsed.slice(1).filter((r) => !r.every((c) => /^:?-{3,}:?$/.test(c)));
      out.push({ type: "table", head: parsed[0], rows: body });
      continue;
    }
    if (/^\s*[-*]\s+/.test(line)) {
      const items: string[] = [];
      while (i < lines.length && /^\s*[-*]\s+/.test(lines[i])) { items.push(lines[i].replace(/^\s*[-*]\s+/, "").trim()); i++; }
      out.push({ type: "list", items });
      continue;
    }
    const para: string[] = [];
    while (i < lines.length && lines[i].trim() && !/^(#{1,6}\s|\||\s*[-*]\s+)/.test(lines[i])) { para.push(lines[i].trim()); i++; }
    out.push({ type: "para", text: para.join(" ") });
  }
  return out;
}

/** The open inbox item that approves this feature's SRS. */
export function srsApprovalItem(items: any[], featureId: string) {
  return items.find((i) => i.kind === "approval" && i.action === "srs_approval" && i.feature_id === featureId);
}
