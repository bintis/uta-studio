import { useCallback, useEffect, useRef } from "react";

const STICK_DEADZONE = 0.5;
const STICK_INITIAL_DELAY = 0.4;
const STICK_REPEAT_RATE = 0.08;

type StickAxis = "up" | "down" | "left" | "right";

interface RepeatState {
  axis: StickAxis | null;
  timer: number;
  started: boolean;
}

export interface GamepadSnapshot {
  up: boolean;
  down: boolean;
  left: boolean;
  right: boolean;
  confirm: boolean;
  back: boolean;
  upHeld: boolean;
  downHeld: boolean;
}

const EMPTY_SNAPSHOT: GamepadSnapshot = {
  up: false,
  down: false,
  left: false,
  right: false,
  confirm: false,
  back: false,
  upHeld: false,
  downHeld: false,
};

const INITIAL_REPEAT: RepeatState = { axis: null, timer: 0, started: false };

function tickRepeat(
  rep: RepeatState,
  dir: StickAxis | null,
  dt: number,
): { up: boolean; down: boolean; left: boolean; right: boolean } {
  const result = { up: false, down: false, left: false, right: false };

  if (dir) {
    if (rep.axis !== dir) {
      rep.axis = dir;
      rep.timer = STICK_INITIAL_DELAY;
      rep.started = true;
      result[dir] = true;
    } else if (rep.started) {
      rep.timer -= dt;
      if (rep.timer <= 0) {
        rep.timer = STICK_REPEAT_RATE;
        result[dir] = true;
      }
    }
  } else {
    rep.axis = null;
    rep.started = false;
  }

  return result;
}

export function useGamepad(onSnapshot: (snap: GamepadSnapshot) => void) {
  const lastTime = useRef(0);
  const callbackRef = useRef(onSnapshot);
  const prevButtons = useRef<Map<number, Set<number>>>(new Map());
  const stickRepeat = useRef<RepeatState>({ ...INITIAL_REPEAT });
  const dpadRepeat = useRef<RepeatState>({ ...INITIAL_REPEAT });

  const poll = useCallback((time: number) => {
    const dt = lastTime.current === 0 ? 0 : (time - lastTime.current) / 1000;
    lastTime.current = time;

    const gamepads = navigator.getGamepads();
    if (!gamepads) {
      return EMPTY_SNAPSHOT;
    }

    let gpConfirm = false;
    let gpBack = false;
    let gpUpHeld = false;
    let gpDownHeld = false;
    let gpLeftHeld = false;
    let gpRightHeld = false;
    let stickDir: StickAxis | null = null;

    for (const gp of gamepads) {
      if (!gp) {
        continue;
      }

      const prev = prevButtons.current.get(gp.index) ?? new Set<number>();
      const curr = new Set<number>();

      for (let i = 0; i < gp.buttons.length; i++) {
        if (gp.buttons[i].pressed) curr.add(i);
      }

      const justPressed = (idx: number) => curr.has(idx) && !prev.has(idx);

      // Standard gamepad mapping:
      // 0=South(A), 1=East(B), 9=Start, 12=DPadUp, 13=DPadDown, 14=DPadLeft, 15=DPadRight
      gpConfirm = gpConfirm || justPressed(0);
      gpBack = gpBack || justPressed(1) || justPressed(9);

      gpUpHeld = gpUpHeld || curr.has(12);
      gpDownHeld = gpDownHeld || curr.has(13);
      gpLeftHeld = gpLeftHeld || curr.has(14);
      gpRightHeld = gpRightHeld || curr.has(15);

      // Left stick
      if (gp.axes.length >= 2) {
        const x = gp.axes[0];
        const y = gp.axes[1];
        // Y axis: negative = up, positive = down (standard mapping)
        if (Math.abs(y) > STICK_DEADZONE || Math.abs(x) > STICK_DEADZONE) {
          // Vertical priority when both exceed deadzone
          if (Math.abs(y) >= Math.abs(x)) {
            stickDir = y < 0 ? "up" : "down";
          } else {
            stickDir = x < 0 ? "left" : "right";
          }
        }
      }

      prevButtons.current.set(gp.index, curr);
    }

    let dpadDir: StickAxis | null = null;
    if (gpUpHeld) dpadDir = "up";
    else if (gpDownHeld) dpadDir = "down";
    else if (gpLeftHeld) dpadDir = "left";
    else if (gpRightHeld) dpadDir = "right";

    const dpad = tickRepeat(dpadRepeat.current, dpadDir, dt);
    const stick = tickRepeat(stickRepeat.current, stickDir, dt);

    const stickUpHeld = stickDir === "up";
    const stickDownHeld = stickDir === "down";

    const snap: GamepadSnapshot = {
      up: dpad.up || stick.up,
      down: dpad.down || stick.down,
      left: dpad.left || stick.left,
      right: dpad.right || stick.right,
      confirm: gpConfirm,
      back: gpBack,
      upHeld: gpUpHeld || stickUpHeld,
      downHeld: gpDownHeld || stickDownHeld,
    };

    return snap;
  }, []);

  useEffect(() => {
    let rafId: number;

    const loop = (time: number) => {
      const snap = poll(time);
      const hasAction =
        snap.up ||
        snap.down ||
        snap.left ||
        snap.right ||
        snap.confirm ||
        snap.back ||
        snap.upHeld ||
        snap.downHeld;

      if (hasAction) {
        callbackRef.current(snap);
      }

      rafId = requestAnimationFrame(loop);
    };

    rafId = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(rafId);
  }, [poll]);
}
