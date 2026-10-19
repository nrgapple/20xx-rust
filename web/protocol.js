// Versioned, ROM-independent experiment protocol. Not a gameplay protocol.
export const MAX_PLAYERS = 4;
export const PROFILE = "fall-fixture-v1";
export const ROOM_ID = /^[a-f0-9]{32}$/;
export function validTrial(value) {
  return value && value.profile === PROFILE &&
    (value.seat === undefined || (Number.isInteger(value.seat) && value.seat >= 1 && value.seat <= MAX_PLAYERS)) &&
    Number.isFinite(value.stick) && value.stick >= -1 && value.stick <= 1 &&
    Number.isInteger(value.flick) && value.flick >= -1 && value.flick <= 599 &&
    Number.isInteger(value.frames) && value.frames >= 10 && value.frames <= 600;
}
export function cleanTrial(value) {
  return { profile: PROFILE, stick: value.stick, flick: value.flick, frames: value.frames, ...(value.seat === undefined ? {} : { seat: value.seat }) };
}
export function trialLink(trials, base) {
  const url = new URL(base);
  url.search = "";
  url.hash = new URLSearchParams({ trials: JSON.stringify(trials.map(cleanTrial)) });
  return url.href;
}
export function parseTrials(hash) {
  const raw = new URLSearchParams(hash.replace(/^#/, "")).get("trials");
  if (raw === null) return null;
  if (raw.length > 1024) throw new Error("This experiment link is too long.");
  const trials = JSON.parse(raw);
  if (!Array.isArray(trials) || trials.length < 1 || trials.length > MAX_PLAYERS || !trials.every(validTrial)) {
    throw new Error("This experiment link is invalid or uses an unsupported version.");
  }
  return trials.map(cleanTrial);
}
