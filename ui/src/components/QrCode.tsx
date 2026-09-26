/** Rows of `1` (dark) / `0` modules as an SVG path: one rectangle per run of
 * dark modules, with the standard four-module quiet zone. */
export function qrPath(rows: string[], quiet = 4): string {
  let path = "";
  rows.forEach((row, y) => {
    let x = 0;
    while (x < row.length) {
      if (row[x] !== "1") {
        x++;
        continue;
      }
      const start = x;
      while (x < row.length && row[x] === "1") x++;
      path += `M${start + quiet} ${y + quiet}h${x - start}v1h-${x - start}z`;
    }
  });
  return path;
}

/** A QR code from the engine's module matrix (`/api/remote/pair`). Always
 * dark on white so phone cameras read it in either theme. */
export function QrCode({ rows, label }: { rows: string[]; label: string }) {
  const total = rows.length + 8;
  return (
    <svg
      className="qr-code"
      viewBox={`0 0 ${total} ${total}`}
      role="img"
      aria-label={label}
      shapeRendering="crispEdges"
    >
      <rect width={total} height={total} fill="#ffffff" />
      <path d={qrPath(rows)} fill="#000000" />
    </svg>
  );
}
