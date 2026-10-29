// Where a card dragged somewhere goes now: where it was left if that's
// still on a monitor, else pulled back onto the nearest one. No Shell
// imports, so it can be tested on its own.

const MARGIN = 8;

// `pos` [x, y], `size` [width, height], `monitors` [{x, y, width, height}].
export function onScreen(pos, size, monitors) {
    if (!monitors.length)
        return pos;
    const [x, y] = pos;
    const [w, h] = size;
    // The monitor holding the card's centre, or else the nearest one.
    const cx = x + w / 2;
    const cy = y + h / 2;
    const distance = m => {
        const dx = Math.max(m.x - cx, 0, cx - (m.x + m.width));
        const dy = Math.max(m.y - cy, 0, cy - (m.y + m.height));
        return dx * dx + dy * dy;
    };
    const m = monitors.reduce((best, mon) => (distance(mon) < distance(best) ? mon : best));
    const clamp = (v, lo, hi) => Math.min(Math.max(v, lo), Math.max(lo, hi));
    return [
        clamp(x, m.x + MARGIN, m.x + m.width - w - MARGIN),
        clamp(y, m.y + MARGIN, m.y + m.height - h - MARGIN),
    ];
}
