import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useEffect, useRef, useState } from 'react';
import './App.css';

// ─────────────────────────────────────────────────────────────────────────────
// Types
// ─────────────────────────────────────────────────────────────────────────────

type PipelineStatus = 'idle' | 'scanning' | 'thinking' | 'done' | 'error';

// ─────────────────────────────────────────────────────────────────────────────
// Sub-components
// ─────────────────────────────────────────────────────────────────────────────

/** Black 38px header — drag anywhere on it to move the window,
 *  click ✕ to fully quit the app.
 *
 *  Drag is driven by an explicit `onMouseDown` → `startDragging()` call
 *  (NOT `data-tauri-drag-region`).  Reasons:
 *    • `data-tauri-drag-region` can be flaky on NSPanel windows that have
 *      the NonactivatingPanel style mask + an elevated window level — the
 *      built-in handler sometimes no-ops because the panel never becomes
 *      the key window.
 *    • Calling `getCurrentWindow().startDragging()` from a real DOM event
 *      bypasses that quirk and always hands the drag off to AppKit's
 *      `performWindowDragWithEvent:`, which works on NSPanel.
 *    • Using a JS handler lets us call `e.preventDefault()` so the browser
 *      never tries to apply any of its own cursor / selection behaviour,
 *      which keeps the cursor as a plain arrow throughout the drag.
 */
function DragHeader() {
	const handleHeaderMouseDown = (e: React.MouseEvent<HTMLDivElement>) => {
		// Only react to the primary (left) mouse button.
		if (e.button !== 0) return;
		// Don't start a drag if the click landed on the close button.
		if ((e.target as HTMLElement).closest('.close-btn')) return;
		// Suppress default behaviour (text selection, focus shifts, cursor change).
		e.preventDefault();
		// Hand the drag over to AppKit. The promise can be safely ignored;
		// any error just means the window manager refused the drag this frame.
		getCurrentWindow().startDragging().catch(() => {
			/* swallow — non-fatal */
		});
	};

	return (
		<div className="drag-header" onMouseDown={handleHeaderMouseDown}>
			<span className="drag-title">underscreen</span>
			<button
				className="close-btn"
				title="Quit"
				onClick={() => invoke('quit_app')}
			>
				✕
			</button>
		</div>
	);
}

function StatusLine({
	status,
	errorMsg,
}: {
	status: PipelineStatus;
	errorMsg: string;
}) {
	switch (status) {
		case 'idle':
			return;
		case 'scanning':
			return (
				<div className="status-active">
					<span className="status-dot status-dot--scan" />
					<span className="status-label">Scanning screen…</span>
				</div>
			);
		case 'thinking':
			return (
				<div className="status-active">
					<span className="status-dot status-dot--think" />
					<span className="status-label">Generating answer…</span>
				</div>
			);
		case 'done':
			return (
				<div className="status-active">
					<span className="status-dot status-dot--done" />
					<span className="status-label">
						Done — press ⌘ ⇧ S to capture again
					</span>
				</div>
			);
		case 'error':
			return (
				<div className="status-active">
					<span className="status-dot status-dot--err" />
					<span className="status-label status-label--err">{errorMsg}</span>
				</div>
			);
	}
}

function HotkeyRow({
	keys,
	label,
	dimmed = false,
}: {
	keys: string[];
	label: string;
	dimmed?: boolean;
}) {
	return (
		<div className={`hotkey-row ${dimmed ? 'hotkey-row--dimmed' : ''}`}>
			<div className="key-group">
				{keys.map((k, i) => (
					<kbd key={i} className="key">
						{k}
					</kbd>
				))}
			</div>
			<span className="hotkey-label">{label}</span>
		</div>
	);
}

// ─────────────────────────────────────────────────────────────────────────────
// Main overlay component
// ─────────────────────────────────────────────────────────────────────────────

export default function App() {
	const [status, setStatus] = useState<PipelineStatus>('idle');
	const [answer, setAnswer] = useState('');
	// key used to force-remount the <pre> when a new capture starts
	const [answerKey, setAnswerKey] = useState(0);
	const [errorMsg, setErrorMsg] = useState('');
	const answerRef = useRef<HTMLPreElement>(null);
	const panelRef = useRef<HTMLDivElement>(null);
	const lastHRef = useRef<number>(0);

	// ── Auto-scroll answer to bottom as tokens stream in ───────────────────
	useEffect(() => {
		if (answerRef.current) {
			answerRef.current.scrollTop = answerRef.current.scrollHeight;
		}
	}, [answer]);

	// ── Resize the OS window to match the panel height ─────────────────────
	// ResizeObserver fires whenever the panel grows (answer streaming in)
	// or shrinks (answer cleared). We clamp: min 150 px, max 390 px (~10 cm).
	// Only invoke when the height actually changes to avoid IPC spam.
	useEffect(() => {
		const el = panelRef.current;
		if (!el) return;

		const ro = new ResizeObserver(([entry]) => {
			const h = Math.ceil(entry.contentRect.height);
			const clamped = Math.max(150, Math.min(h, 390));
			if (Math.abs(clamped - lastHRef.current) > 1) {
				lastHRef.current = clamped;
				invoke('resize_window', { height: clamped });
			}
		});

		ro.observe(el);
		return () => ro.disconnect();
	}, []);

	// ── Rust → React event bus ──────────────────────────────────────────────
	useEffect(() => {
		const subs = Promise.all([
			// Pipeline begins: clear previous answer and show scanning state.
			listen('capture-start', () => {
				setStatus('scanning');
				setAnswer('');
				setErrorMsg('');
				// bump key to remount the answer box so no visual remnants remain
				setAnswerKey((k) => k + 1);
			}),

			// OCR done, LLM stream is starting: switch to thinking state.
			listen('llm-start', () => {
				setStatus('thinking');
			}),

			// Individual token from the LLM stream.
			listen<string>('llm-token', (e) => {
				setAnswer((prev) => prev + e.payload);
			}),

			// Stream finished successfully.
			listen('llm-done', () => {
				setStatus('done');
			}),

			// Any stage of the pipeline failed.
			listen<string>('pipeline-error', (e) => {
				setStatus('error');
				setErrorMsg(e.payload);
			}),
		]);

		return () => {
			subs.then((fns) => fns.forEach((f) => f()));
		};
	}, []);

	const hasAnswer = answer.length > 0;
	const isStreaming = status === 'thinking';

	return (
		// overlay-root: full-window transparent pass-through canvas
		<div className="overlay-root">
			<div
				ref={panelRef}
				className={`panel ${hasAnswer ? 'panel--answer' : 'panel--idle'}`}
			>
				{/* ── Draggable black header — always visible ─────────────── */}
				<DragHeader />

				{/* ── Panel body ──────────────────────────────────────────── */}
				<div className="panel-body">
					{hasAnswer ? (
						<pre key={answerKey} ref={answerRef} className="answer-text">
							{answer}
							{isStreaming && <span className="stream-cursor">▊</span>}
						</pre>
					) : (
						<div className="idle-stack">
							{/* ── Pipeline status ───────────────────────────── */}
							<div className="status-row">
								<StatusLine status={status} errorMsg={errorMsg} />
							</div>

							{/* ── Hotkey reference strip ────────────────────── */}
							<div className="hotkeys">
								<HotkeyRow keys={['⌘', '⇧', 'Space']} label="Hide / Show" />
								<HotkeyRow keys={['⌘', '⇧', 'S']} label="Capture + Answer" />
								<HotkeyRow keys={['⌘', '⌥', '←']} label="Top-left" />
								<HotkeyRow keys={['⌘', '⌥', '→']} label="Top-right" />
								<HotkeyRow keys={['⌘', '⌥', '↑']} label="Top-middle" />
								<HotkeyRow keys={['⌘', '⌥', '↓']} label="Bottom-middle" />
								<HotkeyRow keys={['⌘', '⌥', '⇧', '←']} label="Bottom-left" />
								<HotkeyRow keys={['⌘', '⌥', '⇧', '→']} label="Bottom-right" />
								<HotkeyRow keys={['⌘', 'OPT', 'X']} label="Quit" />
							</div>
						</div>
					)}
				</div>
			</div>
		</div>
	);
}
