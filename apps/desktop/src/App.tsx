import { listen } from '@tauri-apps/api/event';
import { useEffect, useRef, useState } from 'react';
import './App.css';

// ─────────────────────────────────────────────────────────────────────────────
// Types
// ─────────────────────────────────────────────────────────────────────────────

type PipelineStatus = 'idle' | 'scanning' | 'thinking' | 'done' | 'error';

// ─────────────────────────────────────────────────────────────────────────────
// Sub-components (all pointer-events: none via CSS)
// ─────────────────────────────────────────────────────────────────────────────

function StatusLine({
	status,
	errorMsg,
}: {
	status: PipelineStatus;
	errorMsg: string;
}) {
	switch (status) {
		case 'idle':
			return <span className="status-idle">Press ⌘ ⇧ S to capture screen</span>;
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

	// ── Auto-scroll answer to bottom as tokens stream in ───────────────────
	useEffect(() => {
		if (answerRef.current) {
			answerRef.current.scrollTop = answerRef.current.scrollHeight;
		}
	}, [answer]);

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
			<div className={`panel ${hasAnswer ? 'panel--answer' : ''}`}>
				{hasAnswer ? (
					<pre
						key={answerKey}
						ref={answerRef}
						className="answer-text"
						/* scrollTop driven programmatically via ref when answer
               exceeds max-height — otherwise grows naturally with content */
					>
						{answer}
						{isStreaming && <span className="stream-cursor">▊</span>}
					</pre>
				) : (
					<>
						{/* ── Header ──────────────────────────────────────────────────── */}
						<div className="panel-header">
							<div className="panel-logo">
								{/* <span className="logo-icon">◈</span> */}
								{/* <span className="logo-text">UnderScreen</span> */}
							</div>
							{/* <div className="panel-badge">STEALTH</div> */}
						</div>

						<div className="divider" />

						{/* ── Pipeline status ──────────────────────────────────────────── */}
						<div className="status-row">
							<StatusLine status={status} errorMsg={errorMsg} />
						</div>

						<div className="divider" />

						{/* ── Hotkey reference strip ───────────────────────────────────── */}
						<div className="hotkeys">
							<HotkeyRow keys={['⌘', '⇧', 'Space']} label="Hide / Show" />
							<HotkeyRow keys={['⌘', '⇧', 'S']} label="Capture + Answer" />
							<HotkeyRow keys={['⌘', 'OPT', 'X']} label="Quit" dimmed />
						</div>
					</>
				)}
			</div>
		</div>
	);
}
