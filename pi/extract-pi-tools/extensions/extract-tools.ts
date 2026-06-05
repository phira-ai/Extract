import type { ExtensionAPI, ExtensionContext } from "@mariozechner/pi-coding-agent";
import { Type } from "@mariozechner/pi-ai";
import { constants } from "node:fs";
import { access, mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { delimiter, join } from "node:path";

const MAX_OUTPUT_BYTES = 8 * 1024;
const MAX_OUTPUT_LINES = 200;
const FULL_JSON_SAVE_THRESHOLD_BYTES = 12 * 1024;
const EXEC_TIMEOUT_MS = 30_000;
const DEFAULT_LIST_LIMIT = 10;
const MAX_LIST_LIMIT = 25;

type JsonRecord = Record<string, unknown>;
type ExtractParams = Record<string, unknown>;
type Summarizer = (data: unknown, params: ExtractParams) => unknown;

type ExtractExecutable = {
	command: string;
	prefixArgs: string[];
};

async function isExecutable(path: string): Promise<boolean> {
	try {
		await access(path, constants.X_OK);
		return true;
	} catch {
		return false;
	}
}

async function findOnPath(binary: string): Promise<string | undefined> {
	const pathValue = process.env.PATH ?? "";
	for (const dir of pathValue.split(delimiter)) {
		if (!dir) continue;
		const candidate = join(dir, binary);
		if (await isExecutable(candidate)) return candidate;
	}
	return undefined;
}

async function resolveExtract(ctx: ExtensionContext): Promise<ExtractExecutable> {
	const venvExtract = join(ctx.cwd, ".venv", "bin", "extract");
	if (await isExecutable(venvExtract)) return { command: venvExtract, prefixArgs: [] };

	const pathExtract = await findOnPath("extract");
	if (pathExtract) return { command: pathExtract, prefixArgs: [] };

	const venvPython = join(ctx.cwd, ".venv", "bin", "python");
	if (await isExecutable(venvPython)) return { command: venvPython, prefixArgs: ["-m", "extract"] };

	const pathPython = await findOnPath("python");
	if (pathPython) return { command: pathPython, prefixArgs: ["-m", "extract"] };

	throw new Error("Extract CLI not found. Run inside `nix develop`, or install a wheel with `uv pip install dist/extract_tracker-*.whl`.");
}

function shellQuote(value: string): string {
	return /^[A-Za-z0-9_./:=+-]+$/.test(value) ? value : `'${value.replaceAll("'", `'\\''`)}'`;
}

function truncateText(text: string): { text: string; truncated: boolean } {
	const lines = text.split("\n");
	let output = lines.length > MAX_OUTPUT_LINES ? lines.slice(0, MAX_OUTPUT_LINES).join("\n") : text;
	let truncated = lines.length > MAX_OUTPUT_LINES;

	if (Buffer.byteLength(output, "utf8") > MAX_OUTPUT_BYTES) {
		output = Buffer.from(output, "utf8").subarray(0, MAX_OUTPUT_BYTES).toString("utf8");
		truncated = true;
	}

	if (truncated) {
		output += `\n\n[Summary truncated to ${MAX_OUTPUT_LINES} lines / ${MAX_OUTPUT_BYTES} bytes. Narrow query or use context-mode with CLI command in meta.command.]`;
	}
	return { text: output, truncated };
}

function addCommonArgs(args: string[], params: ExtractParams): string[] {
	const store = typeof params.store === "string" && params.store.length > 0 ? params.store : ".extract";
	return [...args, "--store", store, "--format", "json"];
}

function addOptionalString(args: string[], flag: string, value: unknown): void {
	if (typeof value === "string" && value.length > 0) args.push(flag, value);
}

function addOptionalNumber(args: string[], flag: string, value: unknown): void {
	if (typeof value === "number" && Number.isFinite(value)) args.push(flag, String(value));
}

function addOptionalBool(args: string[], flag: string, value: unknown): void {
	if (value === true) args.push(flag);
}

function toolLimit(value: unknown, fallback = DEFAULT_LIST_LIMIT, max = MAX_LIST_LIMIT): number {
	if (typeof value !== "number" || !Number.isFinite(value)) return fallback;
	return Math.max(1, Math.min(Math.trunc(value), max));
}

function asRecord(value: unknown): JsonRecord {
	return value && typeof value === "object" && !Array.isArray(value) ? (value as JsonRecord) : {};
}

function asArray(value: unknown): unknown[] {
	return Array.isArray(value) ? value : [];
}

function clip(value: unknown, max = 220): unknown {
	if (typeof value !== "string") return value;
	return value.length > max ? `${value.slice(0, max)}…` : value;
}

function compactValue(value: unknown): unknown {
	if (value == null || typeof value === "number" || typeof value === "boolean") return value;
	if (typeof value === "string") return clip(value);
	if (Array.isArray(value)) return { type: "array", length: value.length, preview: value.slice(0, 5).map(compactValue) };
	const obj = asRecord(value);
	const keys = Object.keys(obj);
	return { type: "object", n_keys: keys.length, keys: keys.slice(0, 20) };
}

function limitRecord(record: unknown, maxEntries = 50): JsonRecord {
	const entries = Object.entries(asRecord(record));
	const out: JsonRecord = {};
	for (const [key, value] of entries.slice(0, maxEntries)) out[key] = compactValue(value);
	if (entries.length > maxEntries) out.__truncated__ = `${entries.length - maxEntries} more keys omitted`;
	 return out;
}

function getPath(record: unknown, dottedPath: string): unknown {
	let current: unknown = record;
	for (const part of dottedPath.split(".")) {
		const obj = asRecord(current);
		if (!(part in obj)) return undefined;
		current = obj[part];
	}
	return current;
}

function selectedConfig(config: unknown, keys: unknown): JsonRecord | undefined {
	if (!Array.isArray(keys) || keys.length === 0) return undefined;
	const out: JsonRecord = {};
	for (const key of keys) {
		if (typeof key !== "string" || !key) continue;
		const value = getPath(config, key);
		if (value !== undefined) out[key] = compactValue(value);
	}
	return Object.keys(out).length > 0 ? out : undefined;
}

function summarizeEnvelope(data: unknown, mapper: (item: JsonRecord) => JsonRecord): JsonRecord {
	const obj = asRecord(data);
	const items = asArray(obj.items).map((item) => mapper(asRecord(item)));
	return {
		items,
		shown: items.length,
		total: obj.total,
		truncated: obj.truncated,
		limit_clamped: obj.limit_clamped,
	};
}

function runItem(item: JsonRecord): JsonRecord {
	return {
		id: item.id,
		label: item.label,
		experiment_path: item.experiment_path,
		status: item.status,
		started_at: item.started_at,
		ended_at: item.ended_at,
		tags: item.tags,
		config_summary: item.config_summary,
	};
}

const summarizeExperiments: Summarizer = (data) => summarizeEnvelope(data, (item) => ({
	id: item.id,
	path: item.path,
	name: item.name,
	node_type: item.node_type,
	n_runs: item.n_runs,
}));

const summarizeRuns: Summarizer = (data) => summarizeEnvelope(data, runItem);

const summarizeTodos: Summarizer = (data) => summarizeEnvelope(data, (item) => ({
	id: item.id,
	content: clip(item.content, 180),
	priority: item.priority,
	done: item.done,
	scope_type: item.scope_type,
	scope_id: item.scope_id,
	created_at: item.created_at,
}));

const summarizeModels: Summarizer = (data) => summarizeEnvelope(data, (item) => ({
	id: item.id,
	name: item.name,
	version: item.version,
	run_id: item.run_id,
	framework: item.framework,
	created_at: item.created_at,
	metadata: compactValue(item.metadata),
}));

const summarizeRunDetail: Summarizer = (data, params) => {
	const run = asRecord(data);
	const config = asRecord(run.config);
	const configKeys = Object.keys(config);
	return {
		id: run.id,
		label: run.label,
		experiment_id: run.experiment_id,
		experiment_path: run.experiment_path,
		name: run.name,
		status: run.status,
		started_at: run.started_at,
		ended_at: run.ended_at,
		tags: run.tags,
		notes_preview: clip(run.notes, 500),
		metrics_final: limitRecord(run.metrics_final, 80),
		metrics_available: run.metrics_available,
		run_params: limitRecord(run.run_params, 40),
		config_summary: { n_top_level_keys: configKeys.length, top_level_keys: configKeys.slice(0, 80) },
		selected_config: selectedConfig(config, params.configKeys),
		artifacts: asArray(run.artifacts).slice(0, 20).map((artifact) => {
			const obj = asRecord(artifact);
			return { name: obj.name, kind: obj.kind, step: obj.step, rel_path: obj.rel_path };
		}),
		artifact_count: asArray(run.artifacts).length,
		todos: asArray(run.todos).slice(0, 20).map((todo) => {
			const obj = asRecord(todo);
			return { id: obj.id, content: clip(obj.content, 180), priority: obj.priority, done: obj.done };
		}),
		todo_count: asArray(run.todos).length,
	};
};

const summarizeCompare: Summarizer = (data) => {
	const obj = asRecord(data);
	const metrics = asRecord(obj.metrics);
	const metricsOut: JsonRecord = {};
	for (const [name, metric] of Object.entries(metrics).slice(0, 80)) {
		const metricObj = asRecord(metric);
		metricsOut[name] = {
			direction: metricObj.direction,
			values: limitRecord(metricObj.values, 20),
			ranking: metricObj.ranking,
		};
	}

	const configDiffEntries = Object.entries(asRecord(obj.config_diffs));
	const configDiffs: JsonRecord = {};
	for (const [key, value] of configDiffEntries.slice(0, 40)) configDiffs[key] = limitRecord(value, 20);
	if (configDiffEntries.length > 40) configDiffs.__truncated__ = `${configDiffEntries.length - 40} more config diffs omitted`;

	const curves = asRecord(obj.curves);
	const curveSummary: JsonRecord = {};
	for (const [name, perRun] of Object.entries(curves)) {
		const perRunObj = asRecord(perRun);
		curveSummary[name] = Object.fromEntries(Object.entries(perRunObj).map(([runId, points]) => [runId, asArray(points).length]));
	}

	return {
		runs: asArray(obj.runs),
		metrics: metricsOut,
		metric_count: Object.keys(metrics).length,
		config_diffs: configDiffs,
		config_diff_count: configDiffEntries.length,
		curve_point_counts: Object.keys(curveSummary).length > 0 ? curveSummary : undefined,
	};
};

const summarizeLineage: Summarizer = (data) => {
	const obj = asRecord(data);
	return {
		root: obj.root,
		nodes: asArray(obj.nodes).slice(0, 50),
		node_count: asArray(obj.nodes).length,
		edges: asArray(obj.edges).slice(0, 80),
		edge_count: asArray(obj.edges).length,
	};
};

async function saveFullJson(toolName: string, rawJson: string): Promise<string> {
	const dir = await mkdtemp(join(tmpdir(), "extract-pi-"));
	const path = join(dir, `${toolName}.json`);
	await writeFile(path, rawJson, "utf8");
	return path;
}

async function runExtractCli(
	pi: ExtensionAPI,
	ctx: ExtensionContext,
	signal: AbortSignal | undefined,
	toolName: string,
	cliArgs: string[],
	params: ExtractParams,
	summarize: Summarizer,
) {
	const executable = await resolveExtract(ctx);
	const fullArgs = [...executable.prefixArgs, ...cliArgs];
	const result = await pi.exec(executable.command, fullArgs, { signal, timeout: EXEC_TIMEOUT_MS });
	const command = [executable.command, ...fullArgs].map(shellQuote).join(" ");

	if (result.code !== 0) {
		const message = [
			`extract command failed (${result.code}): ${command}`,
			result.stderr.trim(),
			result.stdout.trim(),
		].filter(Boolean).join("\n");
		throw new Error(message);
	}

	let parsed: unknown;
	try {
		parsed = JSON.parse(result.stdout);
	} catch (error) {
		throw new Error(`extract returned non-JSON output for: ${command}\n${String(error)}\n${result.stdout.slice(0, 2000)}`);
	}

	const rawJsonBytes = Buffer.byteLength(result.stdout, "utf8");
	const fullJsonPath = rawJsonBytes > FULL_JSON_SAVE_THRESHOLD_BYTES ? await saveFullJson(toolName, result.stdout) : undefined;
	const payload = {
		summary: summarize(parsed, params),
		meta: {
			command,
			raw_json_bytes: rawJsonBytes,
			full_json_path: fullJsonPath,
			note: fullJsonPath
				? "Native Extract Pi tools return compact summaries. Use ctx_execute_file on full_json_path, or rerun meta.command inside ctx_execute, for custom large analysis."
				: "Native Extract Pi tools return compact summaries. For custom large analysis, rerun meta.command inside ctx_execute and print only the derived answer.",
		},
	};

	const pretty = JSON.stringify(payload, null, 2);
	const truncated = truncateText(pretty);
	return {
		content: [{ type: "text" as const, text: truncated.text }],
		details: { command, rawJsonBytes, fullJsonPath, truncated: truncated.truncated },
	};
}

function registerExtractTool(
	pi: ExtensionAPI,
	definition: {
		name: string;
		label: string;
		description: string;
		parameters: unknown;
		buildArgs(params: ExtractParams): string[];
		summarize: Summarizer;
	},
): void {
	pi.registerTool({
		name: definition.name,
		label: definition.label,
		description: `${definition.description} Returns compact summaries only; use context-mode with meta.command for large/raw analysis.`,
		parameters: definition.parameters as never,
		async execute(_toolCallId, params: ExtractParams, signal, _onUpdate, ctx) {
			return runExtractCli(pi, ctx, signal, definition.name, definition.buildArgs(params), params, definition.summarize);
		},
	});
}

const common = {
	store: Type.Optional(Type.String({ description: "Path to .extract/ directory. Defaults to .extract." })),
};

export default function (pi: ExtensionAPI): void {
	registerExtractTool(pi, {
		name: "extract_list_experiments",
		label: "Extract Experiments",
		description: "List Extract experiments with run counts from a local .extract store.",
		parameters: Type.Object({
			...common,
			prefix: Type.Optional(Type.String({ description: "Experiment path prefix." })),
			limit: Type.Optional(Type.Integer({ minimum: 1, maximum: MAX_LIST_LIMIT, description: "Max rows shown by native tool." })),
			includeArchived: Type.Optional(Type.Boolean({ description: "Include archived experiments and runs." })),
		}),
		buildArgs(params) {
			const args = ["experiments", "list"];
			addOptionalString(args, "--prefix", params.prefix);
			addOptionalNumber(args, "--limit", toolLimit(params.limit));
			addOptionalBool(args, "--include-archived", params.includeArchived);
			return addCommonArgs(args, params);
		},
		summarize: summarizeExperiments,
	});

	registerExtractTool(pi, {
		name: "extract_list_runs",
		label: "Extract Runs",
		description: "List Extract runs, optionally scoped to one experiment.",
		parameters: Type.Object({
			...common,
			experimentId: Type.Optional(Type.String({ description: "Experiment id to scope runs." })),
			limit: Type.Optional(Type.Integer({ minimum: 1, maximum: MAX_LIST_LIMIT, description: "Max rows shown by native tool." })),
			includeArchived: Type.Optional(Type.Boolean({ description: "Include archived runs." })),
		}),
		buildArgs(params) {
			const args = ["runs", "list"];
			addOptionalString(args, "--experiment-id", params.experimentId);
			addOptionalNumber(args, "--limit", toolLimit(params.limit));
			addOptionalBool(args, "--include-archived", params.includeArchived);
			return addCommonArgs(args, params);
		},
		summarize: summarizeRuns,
	});

	registerExtractTool(pi, {
		name: "extract_get_run",
		label: "Extract Run Summary",
		description: "Get compact detail for one Extract run: metrics, config keys, selected config values, params, artifacts, TODOs.",
		parameters: Type.Object({
			...common,
			runId: Type.String({ description: "Run ULID." }),
			configKeys: Type.Optional(Type.Array(Type.String({ description: "Dot-path config key to include, e.g. training.lr." }), { description: "Specific config values to include. Full config is never returned by native tool." })),
		}),
		buildArgs(params) {
			return addCommonArgs(["runs", "get", String(params.runId)], params);
		},
		summarize: summarizeRunDetail,
	});

	registerExtractTool(pi, {
		name: "extract_compare_runs",
		label: "Extract Compare Runs",
		description: "Compare 2-10 Extract runs by final metrics, rankings, and compact config diffs.",
		parameters: Type.Object({
			...common,
			runIds: Type.Array(Type.String({ description: "Run ULID." }), { minItems: 2, maxItems: 10 }),
		}),
		buildArgs(params) {
			return addCommonArgs(["runs", "compare", ...((params.runIds as string[] | undefined) ?? [])], params);
		},
		summarize: summarizeCompare,
	});

	registerExtractTool(pi, {
		name: "extract_search",
		label: "Extract Search",
		description: "Search Extract runs by text plus tag/status/experiment/date filters.",
		parameters: Type.Object({
			...common,
			query: Type.Optional(Type.String({ description: "Text matched against run name, tags, notes." })),
			tag: Type.Optional(Type.String({ description: "Require tag." })),
			status: Type.Optional(Type.String({ description: "Require status: running, completed, failed, archived." })),
			experimentPrefix: Type.Optional(Type.String({ description: "Experiment path prefix." })),
			startedAfter: Type.Optional(Type.String({ description: "ISO timestamp lower bound." })),
			startedBefore: Type.Optional(Type.String({ description: "ISO timestamp upper bound." })),
			limit: Type.Optional(Type.Integer({ minimum: 1, maximum: MAX_LIST_LIMIT, description: "Max rows shown by native tool." })),
			includeArchived: Type.Optional(Type.Boolean({ description: "Include archived runs." })),
		}),
		buildArgs(params) {
			const args = ["search"];
			addOptionalString(args, "--query", params.query);
			addOptionalString(args, "--tag", params.tag);
			addOptionalString(args, "--status", params.status);
			addOptionalString(args, "--experiment-prefix", params.experimentPrefix);
			addOptionalString(args, "--started-after", params.startedAfter);
			addOptionalString(args, "--started-before", params.startedBefore);
			addOptionalNumber(args, "--limit", toolLimit(params.limit));
			addOptionalBool(args, "--include-archived", params.includeArchived);
			return addCommonArgs(args, params);
		},
		summarize: summarizeRuns,
	});

	registerExtractTool(pi, {
		name: "extract_list_todos",
		label: "Extract TODOs",
		description: "List Extract TODOs scoped global, experiment, or run.",
		parameters: Type.Object({
			...common,
			scopeType: Type.Optional(Type.String({ description: "global, experiment, or run. Defaults to global." })),
			scopeId: Type.Optional(Type.String({ description: "Required for experiment/run scope." })),
			includeDone: Type.Optional(Type.Boolean({ description: "Include completed TODOs." })),
			limit: Type.Optional(Type.Integer({ minimum: 1, maximum: MAX_LIST_LIMIT, description: "Max rows shown by native tool." })),
		}),
		buildArgs(params) {
			const args = ["todos", "list"];
			addOptionalString(args, "--scope-type", params.scopeType);
			addOptionalString(args, "--scope-id", params.scopeId);
			addOptionalBool(args, "--include-done", params.includeDone);
			addOptionalNumber(args, "--limit", toolLimit(params.limit));
			return addCommonArgs(args, params);
		},
		summarize: summarizeTodos,
	});

	registerExtractTool(pi, {
		name: "extract_get_lineage",
		label: "Extract Lineage",
		description: "Walk Extract lineage DAG for an experiment, run, or model node.",
		parameters: Type.Object({
			...common,
			nodeType: Type.String({ description: "experiment, run, or model." }),
			nodeId: Type.String({ description: "Node ULID." }),
			direction: Type.Optional(Type.String({ description: "ancestors, descendants, or both. Defaults to both." })),
			depth: Type.Optional(Type.Integer({ minimum: 1, maximum: 5, description: "BFS hop cap." })),
		}),
		buildArgs(params) {
			const args = ["lineage", "get", String(params.nodeType), String(params.nodeId)];
			addOptionalString(args, "--direction", params.direction);
			addOptionalNumber(args, "--depth", params.depth);
			return addCommonArgs(args, params);
		},
		summarize: summarizeLineage,
	});

	registerExtractTool(pi, {
		name: "extract_list_models",
		label: "Extract Models",
		description: "List registered models from an Extract store.",
		parameters: Type.Object({
			...common,
			namePrefix: Type.Optional(Type.String({ description: "Model name prefix." })),
			limit: Type.Optional(Type.Integer({ minimum: 1, maximum: MAX_LIST_LIMIT, description: "Max rows shown by native tool." })),
		}),
		buildArgs(params) {
			const args = ["models", "list"];
			addOptionalString(args, "--name-prefix", params.namePrefix);
			addOptionalNumber(args, "--limit", toolLimit(params.limit));
			return addCommonArgs(args, params);
		},
		summarize: summarizeModels,
	});
}
