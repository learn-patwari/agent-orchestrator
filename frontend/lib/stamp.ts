const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const two = (n: number) => String(n).padStart(2, "0");

/** When a chat message was written, for the small line under it: the time today, the day and time before that, the year only for another year. `at` is seconds since 1970. */
export function stamp(at: number | undefined, now: Date = new Date()): string {
  if (typeof at !== "number" || !Number.isFinite(at)) return "";
  const d = new Date(at * 1000);
  const time = `${two(d.getHours())}:${two(d.getMinutes())}`;
  if (d.toDateString() === now.toDateString()) return time;
  const day = `${d.getDate()} ${MONTHS[d.getMonth()]}`;
  return d.getFullYear() === now.getFullYear() ? `${day}, ${time}` : `${day} ${d.getFullYear()}, ${time}`;
}
