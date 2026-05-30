/**
 * OSX-style window chrome: drag, resize, snap zones, minimize, maximize, close.
 * Vanilla TS — no framework. Calls onResize() after any size change so the
 * embedded xterm can re-fit.
 */

type SnapZone = "top" | "left" | "right" | null;

const MIN_W = 360;
const MIN_H = 200;
const SNAP_EDGE = 24;
const FALLBACK_URL = "https://5cotts.zo.space/";

export interface WindowChromeOptions {
  onResize?: () => void;
}

export function initWindowChrome(opts: WindowChromeOptions = {}) {
  const windowEl = document.getElementById("window") as HTMLDivElement;
  const titlebar = document.getElementById("titlebar") as HTMLDivElement;
  const closeBtn = document.querySelector(".traffic-close") as HTMLButtonElement;
  const minBtn = document.querySelector(".traffic-min") as HTMLButtonElement;
  const maxBtn = document.querySelector(".traffic-max") as HTMLButtonElement;
  const restoreChip = document.getElementById("restore-chip") as HTMLButtonElement;
  const snapOverlay = document.getElementById("snap-overlay") as HTMLDivElement;

  const isTouch = window.matchMedia("(hover: none) and (pointer: coarse)").matches;

  let snap: SnapZone = null;
  let minimized = false;
  let pendingSnap: SnapZone = null;
  let closing = false;

  // ---- helpers --------------------------------------------------------------
  const fit = () => opts.onResize?.();

  const fitSoon = () => {
    requestAnimationFrame(() => requestAnimationFrame(fit));
  };

  const centerWindow = () => {
    if (isTouch || snap !== null) return;
    const defaultW = Math.min(900, Math.max(MIN_W, window.innerWidth - 48));
    const defaultH = Math.min(560, Math.max(MIN_H, window.innerHeight - 48));
    windowEl.style.width = `${defaultW}px`;
    windowEl.style.height = `${defaultH}px`;
    windowEl.style.left = `${Math.max(16, (window.innerWidth - defaultW) / 2)}px`;
    windowEl.style.top = `${Math.max(16, (window.innerHeight - defaultH) / 2)}px`;
  };

  centerWindow();
  fitSoon();

  const applySnap = (zone: SnapZone) => {
    snap = zone;
    windowEl.classList.remove("snap-top", "snap-left", "snap-right");
    if (zone) windowEl.classList.add(`snap-${zone}`);
    fitSoon();
    setTimeout(fit, 300);
  };

  const setPendingSnap = (zone: SnapZone) => {
    if (zone === pendingSnap) return;
    pendingSnap = zone;
    if (!zone) {
      snapOverlay.hidden = true;
      snapOverlay.style.cssText = "";
      return;
    }
    snapOverlay.hidden = false;
    if (zone === "top") {
      snapOverlay.style.cssText =
        "top: 16px; left: 16px; right: 16px; bottom: 16px;";
    } else if (zone === "left") {
      snapOverlay.style.cssText = "top: 0; left: 0; bottom: 0; width: 50vw;";
    } else {
      snapOverlay.style.cssText = "top: 0; right: 0; bottom: 0; width: 50vw;";
    }
  };

  const setMinimized = (m: boolean) => {
    minimized = m;
    windowEl.classList.toggle("minimized", m);
    restoreChip.hidden = !m;
  };

  const handleClose = (e?: Event) => {
    e?.stopPropagation();
    if (closing) return;
    closing = true;
    windowEl.classList.add("minimized");
    setTimeout(() => {
      window.location.href = FALLBACK_URL;
    }, 600);
  };

  // ---- traffic lights -------------------------------------------------------
  closeBtn.addEventListener("click", handleClose);

  minBtn.addEventListener("click", (e) => {
    e.stopPropagation();
    if (isTouch || closing) return;
    setMinimized(true);
  });

  maxBtn.addEventListener("click", (e) => {
    e.stopPropagation();
    if (isTouch || closing) return;
    setMinimized(false);
    applySnap(snap === null ? "top" : null);
  });

  restoreChip.addEventListener("click", (e) => {
    e.stopPropagation();
    setMinimized(false);
    fitSoon();
  });

  // ---- drag from titlebar ---------------------------------------------------
  interface DragState {
    startX: number;
    startY: number;
    baseLeft: number;
    baseTop: number;
    pointerId: number;
  }
  let drag: DragState | null = null;

  const onTitlePointerDown = (e: PointerEvent) => {
    if (isTouch || minimized || closing) return;
    if ((e.target as HTMLElement).closest("[data-traffic]")) return;
    titlebar.setPointerCapture(e.pointerId);
    titlebar.classList.add("dragging");
    windowEl.classList.add("dragging");
    const r = windowEl.getBoundingClientRect();
    // If currently snapped, freeze the visual rect into explicit width/height
    // so the un-snap during move keeps the same size.
    if (snap !== null) {
      windowEl.style.width = `${r.width}px`;
      windowEl.style.height = `${r.height}px`;
      windowEl.style.left = `${r.left}px`;
      windowEl.style.top = `${r.top}px`;
    }
    drag = {
      startX: e.clientX,
      startY: e.clientY,
      baseLeft: r.left,
      baseTop: r.top,
      pointerId: e.pointerId,
    };
  };

  const onTitlePointerMove = (e: PointerEvent) => {
    if (!drag) return;
    const dx = e.clientX - drag.startX;
    const dy = e.clientY - drag.startY;

    if (snap !== null) {
      if (dx * dx + dy * dy < 25) return;
      // Pop out of snap, keep current rect
      const r = windowEl.getBoundingClientRect();
      drag.baseLeft = r.left;
      drag.baseTop = r.top;
      applySnap(null);
    }

    let newLeft = drag.baseLeft + dx;
    let newTop = drag.baseTop + dy;

    // Clamp so the title bar stays at least partially in view
    const w = window.innerWidth;
    const h = window.innerHeight;
    const winW = windowEl.offsetWidth;
    const TITLE_H = 46;
    const MIN_VISIBLE = 80;
    newLeft = Math.max(MIN_VISIBLE - winW, Math.min(w - MIN_VISIBLE, newLeft));
    newTop = Math.max(0, Math.min(h - TITLE_H, newTop));

    windowEl.style.left = `${newLeft}px`;
    windowEl.style.top = `${newTop}px`;

    let zone: SnapZone = null;
    if (e.clientY <= SNAP_EDGE) zone = "top";
    else if (e.clientX <= SNAP_EDGE) zone = "left";
    else if (e.clientX >= w - SNAP_EDGE) zone = "right";
    setPendingSnap(zone);
  };

  const onTitlePointerUp = (e: PointerEvent) => {
    if (!drag) return;
    try {
      titlebar.releasePointerCapture(drag.pointerId);
    } catch {}
    titlebar.classList.remove("dragging");
    windowEl.classList.remove("dragging");
    const snapped = pendingSnap;
    drag = null;
    setPendingSnap(null);
    if (snapped) applySnap(snapped);
    fitSoon();
  };

  titlebar.addEventListener("pointerdown", onTitlePointerDown);
  titlebar.addEventListener("pointermove", onTitlePointerMove);
  titlebar.addEventListener("pointerup", onTitlePointerUp);
  titlebar.addEventListener("pointercancel", onTitlePointerUp);

  titlebar.addEventListener("dblclick", (e) => {
    if (isTouch) return;
    if ((e.target as HTMLElement).closest("[data-traffic]")) return;
    setMinimized(false);
    applySnap(snap === null ? "top" : null);
  });

  // ---- resize handles -------------------------------------------------------
  interface ResizeState {
    edge: string;
    startX: number;
    startY: number;
    startW: number;
    startH: number;
    startLeft: number;
    startTop: number;
    pointerId: number;
    handle: HTMLDivElement;
  }
  let resizeS: ResizeState | null = null;

  document.querySelectorAll<HTMLDivElement>("[data-resize]").forEach((handle) => {
    const edge = handle.dataset.resize!;
    handle.addEventListener("pointerdown", (e) => {
      if (isTouch || minimized || closing) return;
      e.preventDefault();
      e.stopPropagation();
      if (snap !== null) {
        const r = windowEl.getBoundingClientRect();
        windowEl.style.width = `${r.width}px`;
        windowEl.style.height = `${r.height}px`;
        windowEl.style.left = `${r.left}px`;
        windowEl.style.top = `${r.top}px`;
        applySnap(null);
      }
      const r = windowEl.getBoundingClientRect();
      handle.setPointerCapture(e.pointerId);
      windowEl.classList.add("resizing");
      resizeS = {
        edge,
        startX: e.clientX,
        startY: e.clientY,
        startW: r.width,
        startH: r.height,
        startLeft: r.left,
        startTop: r.top,
        pointerId: e.pointerId,
        handle,
      };
    });

    handle.addEventListener("pointermove", (e) => {
      if (!resizeS || resizeS.handle !== handle) return;
      const dx = e.clientX - resizeS.startX;
      const dy = e.clientY - resizeS.startY;
      let newW = resizeS.startW;
      let newH = resizeS.startH;
      let newLeft = resizeS.startLeft;
      let newTop = resizeS.startTop;

      if (resizeS.edge.includes("e")) {
        newW = Math.max(MIN_W, resizeS.startW + dx);
      }
      if (resizeS.edge.includes("s")) {
        newH = Math.max(MIN_H, resizeS.startH + dy);
      }
      if (resizeS.edge.includes("w")) {
        newW = Math.max(MIN_W, resizeS.startW - dx);
        newLeft = resizeS.startLeft + (resizeS.startW - newW);
      }
      if (resizeS.edge.includes("n")) {
        newH = Math.max(MIN_H, resizeS.startH - dy);
        newTop = resizeS.startTop + (resizeS.startH - newH);
      }

      // Clamp to viewport
      newLeft = Math.max(0, Math.min(window.innerWidth - 100, newLeft));
      newTop = Math.max(0, Math.min(window.innerHeight - 60, newTop));

      windowEl.style.width = `${newW}px`;
      windowEl.style.height = `${newH}px`;
      windowEl.style.left = `${newLeft}px`;
      windowEl.style.top = `${newTop}px`;

      fit();
    });

    const endResize = (e: PointerEvent) => {
      if (!resizeS || resizeS.handle !== handle) return;
      try {
        handle.releasePointerCapture(resizeS.pointerId);
      } catch {}
      windowEl.classList.remove("resizing");
      resizeS = null;
      fitSoon();
    };
    handle.addEventListener("pointerup", endResize);
    handle.addEventListener("pointercancel", endResize);
  });

  // ---- viewport resize ------------------------------------------------------
  window.addEventListener("resize", () => {
    if (snap !== null || isTouch) {
      fit();
      return;
    }
    // Keep window inside viewport
    const w = window.innerWidth;
    const h = window.innerHeight;
    const r = windowEl.getBoundingClientRect();
    let left = r.left;
    let top = r.top;
    let width = r.width;
    let height = r.height;
    if (width > w - 16) width = Math.max(MIN_W, w - 16);
    if (height > h - 16) height = Math.max(MIN_H, h - 16);
    if (left + width > w - 8) left = w - 8 - width;
    if (top + height > h - 8) top = h - 8 - height;
    left = Math.max(8, left);
    top = Math.max(8, top);
    windowEl.style.left = `${left}px`;
    windowEl.style.top = `${top}px`;
    windowEl.style.width = `${width}px`;
    windowEl.style.height = `${height}px`;
    fit();
  });

  // ---- escape key to clear snap --------------------------------------------
  window.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && snap !== null) {
      applySnap(null);
      centerWindow();
    }
  });

  return {
    fit: fitSoon,
    isTouch,
  };
}
