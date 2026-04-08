use std::path::Path;

use color_eyre::Result;
use rusqlite::{Connection, params};

use crate::model::{Artifact, Experiment, LineageEdge, MetricAggregate, Model, Run, RunParam, ScalarMetric, Todo};

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA query_only=ON; PRAGMA journal_mode=WAL;")?;
        Ok(Self { conn })
    }

    /// Returns SQLite's `PRAGMA data_version` counter, which increments whenever
    /// any other connection commits to the database file. Cheap (no I/O) — safe
    /// to call from a tight tick loop. Used by the TUI to skip refresh work
    /// when the store hasn't changed.
    pub fn data_version(&self) -> Result<i64> {
        let v: i64 = self
            .conn
            .query_row("PRAGMA data_version", [], |row| row.get(0))?;
        Ok(v)
    }

    /// Returns true if any run in the store is currently in 'running' status.
    /// Used by the TUI status bar to show a global ● LIVE indicator.
    pub fn has_running_runs(&self) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM runs WHERE status = 'running' LIMIT 1",
            [],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    // Experiments

    pub fn list_experiments(&self) -> Result<Vec<Experiment>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, name, parent_id, created_at, metadata, status, node_type FROM experiments ORDER BY created_at",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Experiment {
                id: row.get(0)?,
                path: row.get(1)?,
                name: row.get(2)?,
                parent_id: row.get(3)?,
                created_at: row.get(4)?,
                metadata: row.get(5)?,
                status: row.get(6)?,
                node_type: row.get(7)?,
            })
        })?;
        let mut experiments = Vec::new();
        for row in rows {
            experiments.push(row?);
        }
        Ok(experiments)
    }

    pub fn get_experiment(&self, id: &str) -> Result<Option<Experiment>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, name, parent_id, created_at, metadata, status, node_type FROM experiments WHERE id = ?",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(Experiment {
                id: row.get(0)?,
                path: row.get(1)?,
                name: row.get(2)?,
                parent_id: row.get(3)?,
                created_at: row.get(4)?,
                metadata: row.get(5)?,
                status: row.get(6)?,
                node_type: row.get(7)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    #[allow(dead_code)]
    pub fn list_hierarchy(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT level_name FROM hierarchy ORDER BY level_order",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    // Runs

    pub fn list_runs(&self, experiment_id: &str) -> Result<Vec<Run>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, experiment_id, name, config, started_at, ended_at, status, hostname, git_sha, tags, notes, total_steps FROM runs WHERE experiment_id = ? ORDER BY started_at",
        )?;
        let rows = stmt.query_map(params![experiment_id], |row| {
            Ok(Run {
                id: row.get(0)?,
                experiment_id: row.get(1)?,
                name: row.get(2)?,
                config: row.get(3)?,
                started_at: row.get(4)?,
                ended_at: row.get(5)?,
                status: row.get(6)?,
                hostname: row.get(7)?,
                git_sha: row.get(8)?,
                tags: row.get(9)?,
                notes: row.get(10)?,
                total_steps: row.get(11)?,
            })
        })?;
        let mut runs = Vec::new();
        for row in rows {
            runs.push(row?);
        }
        Ok(runs)
    }

    pub fn get_run(&self, id: &str) -> Result<Option<Run>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, experiment_id, name, config, started_at, ended_at, status, hostname, git_sha, tags, notes, total_steps FROM runs WHERE id = ?",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(Run {
                id: row.get(0)?,
                experiment_id: row.get(1)?,
                name: row.get(2)?,
                config: row.get(3)?,
                started_at: row.get(4)?,
                ended_at: row.get(5)?,
                status: row.get(6)?,
                hostname: row.get(7)?,
                git_sha: row.get(8)?,
                tags: row.get(9)?,
                notes: row.get(10)?,
                total_steps: row.get(11)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    // Metrics

    pub fn get_latest_metrics(&self, run_id: &str) -> Result<Vec<ScalarMetric>> {
        let mut stmt = self.conn.prepare(
            "SELECT sm.id, sm.run_id, sm.step, sm.name, sm.value, sm.wall_time \
             FROM scalar_metrics sm \
             INNER JOIN ( \
                 SELECT run_id, name, MAX(step) as max_step \
                 FROM scalar_metrics WHERE run_id = ? \
                 GROUP BY run_id, name \
             ) latest \
             ON sm.run_id = latest.run_id AND sm.name = latest.name AND sm.step = latest.max_step",
        )?;
        let rows = stmt.query_map(params![run_id], |row| {
            Ok(ScalarMetric {
                id: row.get(0)?,
                run_id: row.get(1)?,
                step: row.get(2)?,
                name: row.get(3)?,
                value: row.get(4)?,
                wall_time: row.get(5)?,
            })
        })?;
        let mut metrics = Vec::new();
        for row in rows {
            metrics.push(row?);
        }
        Ok(metrics)
    }

    /// Distinct metric names that have at least one curve point for this run,
    /// in alphabetical order. Used by the TUI to enumerate curves to render.
    pub fn list_curve_names(&self, run_id: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT name FROM curve_points WHERE run_id = ? ORDER BY name",
        )?;
        let rows = stmt.query_map(params![run_id], |row| row.get::<_, String>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// All curve points for a (run, metric), ordered by step ascending.
    /// Returns (step, value, wall_time) tuples.
    pub fn list_curve_points(
        &self,
        run_id: &str,
        name: &str,
    ) -> Result<Vec<(i64, f64, Option<f64>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT step, value, wall_time FROM curve_points \
             WHERE run_id = ? AND name = ? ORDER BY step",
        )?;
        let rows = stmt.query_map(params![run_id, name], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?, row.get::<_, Option<f64>>(2)?))
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    // Artifacts

    pub fn list_artifacts(&self, run_id: &str) -> Result<Vec<Artifact>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, run_id, name, kind, step, rel_path, shape, dtype, metadata, created_at FROM artifacts WHERE run_id = ? ORDER BY created_at",
        )?;
        let rows = stmt.query_map(params![run_id], |row| {
            Ok(Artifact {
                id: row.get(0)?,
                run_id: row.get(1)?,
                name: row.get(2)?,
                kind: row.get(3)?,
                step: row.get(4)?,
                rel_path: row.get(5)?,
                shape: row.get(6)?,
                dtype: row.get(7)?,
                metadata: row.get(8)?,
                created_at: row.get(9)?,
            })
        })?;
        let mut artifacts = Vec::new();
        for row in rows {
            artifacts.push(row?);
        }
        Ok(artifacts)
    }

    // Run params (categorical/string attributes)

    pub fn list_run_params(&self, run_id: &str) -> Result<Vec<RunParam>> {
        let mut stmt = self.conn.prepare(
            "SELECT run_id, name, value FROM run_params WHERE run_id = ? ORDER BY name",
        )?;
        let rows = stmt.query_map(params![run_id], |row| {
            Ok(RunParam {
                run_id: row.get(0)?,
                name: row.get(1)?,
                value: row.get(2)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    // Models

    pub fn list_models(&self) -> Result<Vec<Model>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, version, run_id, artifact_path, framework, metadata, created_at FROM models ORDER BY name, version",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Model {
                id: row.get(0)?,
                name: row.get(1)?,
                version: row.get(2)?,
                run_id: row.get(3)?,
                artifact_path: row.get(4)?,
                framework: row.get(5)?,
                metadata: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        let mut models = Vec::new();
        for row in rows {
            models.push(row?);
        }
        Ok(models)
    }

    pub fn get_model(&self, id: &str) -> Result<Option<Model>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, version, run_id, artifact_path, framework, metadata, created_at \
             FROM models WHERE id = ?",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(Model {
                id: row.get(0)?,
                name: row.get(1)?,
                version: row.get(2)?,
                run_id: row.get(3)?,
                artifact_path: row.get(4)?,
                framework: row.get(5)?,
                metadata: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    // Lineage

    pub fn list_all_lineage(&self) -> Result<Vec<LineageEdge>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, parent_type, parent_id, child_type, child_id, relation, metadata, created_at \
             FROM lineage ORDER BY created_at",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(LineageEdge {
                id: row.get(0)?,
                parent_type: row.get(1)?,
                parent_id: row.get(2)?,
                child_type: row.get(3)?,
                child_id: row.get(4)?,
                relation: row.get(5)?,
                metadata: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        let mut edges = Vec::new();
        for row in rows {
            edges.push(row?);
        }
        Ok(edges)
    }

    // --- Phase 1.5: Aggregate queries ---

    pub fn count_all_runs(&self) -> Result<i64> {
        let count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM runs", [], |row| row.get(0))?;
        Ok(count)
    }

    pub fn count_leaf_experiments(&self) -> Result<usize> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM experiments e \
             WHERE NOT EXISTS (SELECT 1 FROM experiments c WHERE c.parent_id = e.id)",
            [],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    pub fn recent_runs(&self, limit: i64) -> Result<Vec<Run>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, experiment_id, name, config, started_at, ended_at, status, \
                    hostname, git_sha, tags, notes, total_steps \
             FROM runs ORDER BY started_at DESC LIMIT ?",
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(Run {
                id: row.get(0)?,
                experiment_id: row.get(1)?,
                name: row.get(2)?,
                config: row.get(3)?,
                started_at: row.get(4)?,
                ended_at: row.get(5)?,
                status: row.get(6)?,
                hostname: row.get(7)?,
                git_sha: row.get(8)?,
                tags: row.get(9)?,
                notes: row.get(10)?,
                total_steps: row.get(11)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn count_descendant_experiments(&self, path_prefix: &str) -> Result<i64> {
        let pattern = format!("{path_prefix}/%");
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM experiments e WHERE e.path LIKE ? \
             AND NOT EXISTS (SELECT 1 FROM experiments c WHERE c.parent_id = e.id)",
            params![pattern],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    pub fn count_runs_for_subtree(&self, path_prefix: &str) -> Result<i64> {
        let pattern = format!("{path_prefix}/%");
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM runs r \
             INNER JOIN experiments e ON r.experiment_id = e.id \
             WHERE e.path = ? OR e.path LIKE ?",
            params![path_prefix, pattern],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    pub fn runs_by_status_for_subtree(&self, path_prefix: &str) -> Result<Vec<(String, i64)>> {
        let pattern = format!("{path_prefix}/%");
        let mut stmt = self.conn.prepare(
            "SELECT r.status, COUNT(*) FROM runs r \
             INNER JOIN experiments e ON r.experiment_id = e.id \
             WHERE e.path = ? OR e.path LIKE ? \
             GROUP BY r.status",
        )?;
        let rows = stmt.query_map(params![path_prefix, pattern], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn run_counts_by_child(&self, parent_id: &str) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT e.name, ( \
                 SELECT COUNT(*) FROM runs r \
                 INNER JOIN experiments e2 ON r.experiment_id = e2.id \
                 WHERE e2.path = e.path OR e2.path LIKE e.path || '/%' \
             ) as run_count \
             FROM experiments e \
             WHERE e.parent_id = ? \
             ORDER BY e.name",
        )?;
        let rows = stmt.query_map(params![parent_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Uses population std_dev (not sample) — consistent with ML tracking conventions.
    pub fn aggregate_final_metrics(&self, experiment_id: &str) -> Result<Vec<MetricAggregate>> {
        let mut stmt = self.conn.prepare(
            "SELECT sm.name, \
                    AVG(sm.value), \
                    CASE WHEN COUNT(*) > 1 \
                        THEN AVG(sm.value * sm.value) - AVG(sm.value) * AVG(sm.value) \
                        ELSE 0.0 END, \
                    MIN(sm.value), MAX(sm.value), COUNT(*) \
             FROM scalar_metrics sm \
             INNER JOIN ( \
                 SELECT sm2.run_id, sm2.name, MAX(sm2.step) as max_step \
                 FROM scalar_metrics sm2 \
                 INNER JOIN runs r ON sm2.run_id = r.id \
                 WHERE r.experiment_id = ? \
                 GROUP BY sm2.run_id, sm2.name \
             ) latest ON sm.run_id = latest.run_id \
                     AND sm.name = latest.name \
                     AND sm.step = latest.max_step \
             GROUP BY sm.name ORDER BY sm.name",
        )?;
        let rows = stmt.query_map(params![experiment_id], |row| {
            let variance: f64 = row.get(2)?;
            Ok(MetricAggregate {
                name: row.get(0)?,
                mean: row.get(1)?,
                std_dev: variance.max(0.0).sqrt(),
                min: row.get(3)?,
                max: row.get(4)?,
                count: row.get(5)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// For each direct child of parent_id, get min and max of the final value
    /// of each metric across all runs in that child's subtree.
    /// Returns (child_name, metric_name, min_value, max_value).
    pub fn child_best_metrics(
        &self,
        parent_id: &str,
    ) -> Result<Vec<(String, String, f64, f64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT sub.child_name, sub.metric_name, \
                    MIN(sub.final_value), MAX(sub.final_value) \
             FROM ( \
                 SELECT e.name AS child_name, sm.name AS metric_name, sm.value AS final_value \
                 FROM experiments e \
                 INNER JOIN experiments e2 \
                     ON (e2.path = e.path OR e2.path LIKE e.path || '/%') \
                 INNER JOIN runs r ON r.experiment_id = e2.id \
                 INNER JOIN ( \
                     SELECT run_id, name, MAX(step) AS max_step \
                     FROM scalar_metrics GROUP BY run_id, name \
                 ) latest ON latest.run_id = r.id \
                 INNER JOIN scalar_metrics sm \
                     ON sm.run_id = latest.run_id \
                     AND sm.name = latest.name \
                     AND sm.step = latest.max_step \
                 WHERE e.parent_id = ? \
             ) sub \
             GROUP BY sub.child_name, sub.metric_name \
             ORDER BY sub.metric_name, sub.child_name",
        )?;
        let rows = stmt.query_map(params![parent_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, f64>(2)?,
                row.get::<_, f64>(3)?,
            ))
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn count_unique_configs(&self, experiment_id: &str) -> Result<i64> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(DISTINCT config) FROM runs \
             WHERE experiment_id = ? AND config IS NOT NULL",
            params![experiment_id],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Delete a run and all its associated data.
    /// Opens a separate writable connection since the main one is read-only.
    pub fn delete_run(db_path: &std::path::Path, run_id: &str) -> Result<()> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

        let tx = conn.unchecked_transaction()?;
        tx.execute("DELETE FROM scalar_metrics WHERE run_id = ?", params![run_id])?;
        tx.execute("DELETE FROM curve_points WHERE run_id = ?", params![run_id])?;
        tx.execute("DELETE FROM run_params WHERE run_id = ?", params![run_id])?;
        tx.execute("DELETE FROM artifacts WHERE run_id = ?", params![run_id])?;
        tx.execute("DELETE FROM lineage WHERE (parent_type = 'run' AND parent_id = ?) OR (child_type = 'run' AND child_id = ?)", params![run_id, run_id])?;
        tx.execute("DELETE FROM runs WHERE id = ?", params![run_id])?;
        tx.commit()?;

        Ok(())
    }

    /// Delete an experiment and all its descendants (child experiments, runs, metrics, etc.).
    /// Returns the list of deleted run IDs so callers can clean up artifacts.
    pub fn delete_experiment(db_path: &std::path::Path, experiment_id: &str) -> Result<Vec<String>> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

        // Collect all experiment IDs to delete (the target + all descendants).
        let mut exp_ids: Vec<String> = vec![experiment_id.to_string()];
        let mut i = 0;
        while i < exp_ids.len() {
            let mut stmt = conn.prepare(
                "SELECT id FROM experiments WHERE parent_id = ?",
            )?;
            let children: Vec<String> = stmt
                .query_map(params![exp_ids[i]], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();
            exp_ids.extend(children);
            i += 1;
        }

        // Collect all run IDs under these experiments.
        let mut run_ids: Vec<String> = Vec::new();
        for eid in &exp_ids {
            let mut stmt = conn.prepare("SELECT id FROM runs WHERE experiment_id = ?")?;
            let ids: Vec<String> = stmt
                .query_map(params![eid], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();
            run_ids.extend(ids);
        }

        let tx = conn.unchecked_transaction()?;

        // Delete run data for all collected runs.
        for rid in &run_ids {
            tx.execute("DELETE FROM scalar_metrics WHERE run_id = ?", params![rid])?;
            tx.execute("DELETE FROM curve_points WHERE run_id = ?", params![rid])?;
            tx.execute("DELETE FROM run_params WHERE run_id = ?", params![rid])?;
            tx.execute("DELETE FROM artifacts WHERE run_id = ?", params![rid])?;
            tx.execute("DELETE FROM lineage WHERE (parent_type = 'run' AND parent_id = ?) OR (child_type = 'run' AND child_id = ?)", params![rid, rid])?;
            tx.execute("DELETE FROM todos WHERE scope_type = 'run' AND scope_id = ?", params![rid])?;
        }

        // Delete runs, todos, lineage, and experiments (children first due to FK).
        for eid in exp_ids.iter().rev() {
            tx.execute("DELETE FROM runs WHERE experiment_id = ?", params![eid])?;
            tx.execute("DELETE FROM todos WHERE scope_type = 'experiment' AND scope_id = ?", params![eid])?;
            tx.execute("DELETE FROM lineage WHERE (parent_type = 'experiment' AND parent_id = ?) OR (child_type = 'experiment' AND child_id = ?)", params![eid, eid])?;
            tx.execute("DELETE FROM experiments WHERE id = ?", params![eid])?;
        }

        tx.commit()?;
        Ok(run_ids)
    }

    // Todos

    pub fn list_todos(&self, scope_type: Option<&str>, scope_id: Option<&str>) -> Result<Vec<Todo>> {
        let sql = match (scope_type, scope_id) {
            (Some(_), Some(_)) => {
                "SELECT id, scope_type, scope_id, content, done, priority, created_at, completed_at \
                 FROM todos WHERE scope_type = ? AND scope_id = ? ORDER BY priority DESC, created_at"
            }
            (Some(_), None) => {
                "SELECT id, scope_type, scope_id, content, done, priority, created_at, completed_at \
                 FROM todos WHERE scope_type = ? ORDER BY priority DESC, created_at"
            }
            _ => {
                "SELECT id, scope_type, scope_id, content, done, priority, created_at, completed_at \
                 FROM todos ORDER BY priority DESC, created_at"
            }
        };

        let mut stmt = self.conn.prepare(sql)?;

        let rows: Vec<Todo> = match (scope_type, scope_id) {
            (Some(st), Some(si)) => {
                let mapped = stmt.query_map(params![st, si], |row| {
                    Ok(Todo {
                        id: row.get(0)?,
                        scope_type: row.get(1)?,
                        scope_id: row.get(2)?,
                        content: row.get(3)?,
                        done: row.get::<_, i64>(4)? != 0,
                        priority: row.get(5)?,
                        created_at: row.get(6)?,
                        completed_at: row.get(7)?,
                    })
                })?;
                mapped.collect::<std::result::Result<Vec<_>, _>>()?
            }
            (Some(st), None) => {
                let mapped = stmt.query_map(params![st], |row| {
                    Ok(Todo {
                        id: row.get(0)?,
                        scope_type: row.get(1)?,
                        scope_id: row.get(2)?,
                        content: row.get(3)?,
                        done: row.get::<_, i64>(4)? != 0,
                        priority: row.get(5)?,
                        created_at: row.get(6)?,
                        completed_at: row.get(7)?,
                    })
                })?;
                mapped.collect::<std::result::Result<Vec<_>, _>>()?
            }
            _ => {
                let mapped = stmt.query_map([], |row| {
                    Ok(Todo {
                        id: row.get(0)?,
                        scope_type: row.get(1)?,
                        scope_id: row.get(2)?,
                        content: row.get(3)?,
                        done: row.get::<_, i64>(4)? != 0,
                        priority: row.get(5)?,
                        created_at: row.get(6)?,
                        completed_at: row.get(7)?,
                    })
                })?;
                mapped.collect::<std::result::Result<Vec<_>, _>>()?
            }
        };

        Ok(rows)
    }

    pub fn search(&self, query: &str) -> Result<Vec<crate::model::SearchResult>> {
        let pattern = format!("%{query}%");
        let mut results = Vec::new();

        // Search experiments by path and name
        let mut stmt = self.conn.prepare(
            "SELECT id, path, name FROM experiments WHERE path LIKE ?1 OR name LIKE ?1 LIMIT 20",
        )?;
        let rows = stmt.query_map(params![pattern], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (id, path, name) = row?;
            let matched = if name.to_lowercase().contains(&query.to_lowercase()) {
                ("name", name.clone())
            } else {
                ("path", path.clone())
            };
            results.push(crate::model::SearchResult {
                result_type: "experiment".to_string(),
                id,
                experiment_id: None,
                label: path,
                matched_field: matched.0.to_string(),
            });
        }

        // Search runs by name, tags, notes
        let mut stmt = self.conn.prepare(
            "SELECT r.id, r.experiment_id, r.name, r.tags, r.notes, e.path \
             FROM runs r JOIN experiments e ON r.experiment_id = e.id \
             WHERE r.name LIKE ?1 OR r.tags LIKE ?1 OR r.notes LIKE ?1 LIMIT 20",
        )?;
        let rows = stmt.query_map(params![pattern], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;
        for row in rows {
            let (id, exp_id, name, tags, _notes, exp_path) = row?;
            let q_lower = query.to_lowercase();
            let field = if name.as_deref().unwrap_or("").to_lowercase().contains(&q_lower) {
                "name"
            } else if tags.as_deref().unwrap_or("").to_lowercase().contains(&q_lower) {
                "tags"
            } else {
                "notes"
            };
            let label = name.unwrap_or_else(|| format!("{exp_path} (run)"));
            results.push(crate::model::SearchResult {
                result_type: "run".to_string(),
                id,
                experiment_id: Some(exp_id),
                label,
                matched_field: field.to_string(),
            });
        }

        Ok(results)
    }

    /// Toggle a todo's done status. Opens a writable connection.
    pub fn toggle_todo(db_path: &Path, todo_id: &str) -> Result<bool> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let done: i64 = conn.query_row(
            "SELECT done FROM todos WHERE id = ?",
            params![todo_id],
            |row| row.get(0),
        )?;
        let new_done = if done != 0 { 0 } else { 1 };
        let completed_at = if new_done == 1 {
            Some(chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
        } else {
            None
        };
        conn.execute(
            "UPDATE todos SET done = ?, completed_at = ? WHERE id = ?",
            params![new_done, completed_at, todo_id],
        )?;
        Ok(new_done == 1)
    }

    /// Set a todo's priority. Opens a writable connection.
    pub fn set_todo_priority(db_path: &Path, todo_id: &str, priority: i64) -> Result<()> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        conn.execute(
            "UPDATE todos SET priority = ? WHERE id = ?",
            params![priority, todo_id],
        )?;
        Ok(())
    }

    /// Delete a todo. Opens a writable connection.
    pub fn delete_todo(db_path: &Path, todo_id: &str) -> Result<()> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        conn.execute("DELETE FROM todos WHERE id = ?", params![todo_id])?;
        Ok(())
    }

    /// Add a new todo. Opens a writable connection.
    pub fn add_todo(db_path: &Path, content: &str, priority: i64, scope_type: &str, scope_id: Option<&str>) -> Result<()> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let id = format!("{}", ulid::Ulid::new());
        conn.execute(
            "INSERT INTO todos (id, scope_type, scope_id, content, done, priority) VALUES (?, ?, ?, ?, 0, ?)",
            params![id, scope_type, scope_id, content, priority],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../schema/migrations/001_init.sql"))
            .unwrap();
        conn.execute_batch(
            "INSERT INTO hierarchy VALUES (0, 'benchmark');
             INSERT INTO hierarchy VALUES (1, 'method');
             INSERT INTO hierarchy VALUES (2, 'variant');
             INSERT INTO experiments VALUES ('e_a', 'a', 'a', NULL, '2026-01-01T00:00:00Z', NULL, 'active', 'benchmark');
             INSERT INTO experiments VALUES ('e_b', 'a/b', 'b', 'e_a', '2026-01-01T00:00:00Z', NULL, 'active', 'method');
             INSERT INTO experiments VALUES ('e_c', 'a/c', 'c', 'e_a', '2026-01-01T00:00:00Z', NULL, 'active', 'method');
             INSERT INTO experiments VALUES ('e_d', 'a/d', 'd', 'e_a', '2026-01-01T00:00:00Z', NULL, 'active', 'method');
             INSERT INTO experiments VALUES ('e_e', 'a/d/e', 'e', 'e_d', '2026-01-01T00:00:00Z', NULL, 'active', 'variant');
             INSERT INTO runs VALUES ('r1', 'e_b', 'run1', '{\"lr\": 0.01}', '2026-01-01T00:00:00Z', '2026-01-01T01:00:00Z', 'completed', NULL, NULL, NULL, NULL, NULL);
             INSERT INTO runs VALUES ('r2', 'e_b', 'run2', '{\"lr\": 0.001}', '2026-01-02T00:00:00Z', NULL, 'running', NULL, NULL, NULL, NULL, NULL);
             INSERT INTO runs VALUES ('r3', 'e_c', 'run3', '{\"lr\": 0.01}', '2026-01-03T00:00:00Z', '2026-01-03T01:00:00Z', 'completed', NULL, NULL, NULL, NULL, NULL);
             INSERT INTO runs VALUES ('r4', 'e_e', 'run4', '{\"lr\": 0.1}', '2026-01-04T00:00:00Z', NULL, 'failed', NULL, NULL, NULL, NULL, NULL);
             INSERT INTO scalar_metrics VALUES (NULL, 'r1', 10, 'loss', 0.5, NULL);
             INSERT INTO scalar_metrics VALUES (NULL, 'r1', 20, 'loss', 0.3, NULL);
             INSERT INTO scalar_metrics VALUES (NULL, 'r1', 10, 'accuracy', 0.7, NULL);
             INSERT INTO scalar_metrics VALUES (NULL, 'r1', 20, 'accuracy', 0.85, NULL);
             INSERT INTO scalar_metrics VALUES (NULL, 'r2', 5, 'loss', 0.6, NULL);
             INSERT INTO scalar_metrics VALUES (NULL, 'r2', 5, 'accuracy', 0.65, NULL);
             INSERT INTO scalar_metrics VALUES (NULL, 'r3', 15, 'loss', 0.4, NULL);
             INSERT INTO scalar_metrics VALUES (NULL, 'r3', 15, 'accuracy', 0.8, NULL);
             INSERT INTO scalar_metrics VALUES (NULL, 'r4', 10, 'loss', 0.9, NULL);
             INSERT INTO scalar_metrics VALUES (NULL, 'r4', 10, 'accuracy', 0.4, NULL);
             INSERT INTO run_params VALUES (NULL, 'r1', 'arch', 'resnet18');
             INSERT INTO run_params VALUES (NULL, 'r1', 'fisher_label', 'empirical');
             INSERT INTO curve_points VALUES ('r1', 'train_loss', 0, 1.0, 0.0);
             INSERT INTO curve_points VALUES ('r1', 'train_loss', 1, 0.8, 0.5);
             INSERT INTO curve_points VALUES ('r1', 'train_loss', 2, 0.6, 1.0);
             INSERT INTO curve_points VALUES ('r1', 'lr_schedule', 0, 0.001, 0.0);
             INSERT INTO curve_points VALUES ('r1', 'lr_schedule', 1, 0.0009, 0.5);
             INSERT INTO curve_points VALUES ('r2', 'train_loss', 0, 1.2, 0.0);",
        )
        .unwrap();
        Db { conn }
    }

    #[test]
    fn test_count_all_runs() {
        let db = test_db();
        assert_eq!(db.count_all_runs().unwrap(), 4);
    }

    #[test]
    fn test_recent_runs() {
        let db = test_db();
        let runs = db.recent_runs(2).unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].id, "r4");
        assert_eq!(runs[1].id, "r3");
    }

    #[test]
    fn test_count_descendant_experiments() {
        let db = test_db();
        // Leaves only: b, c, e (d has child e, so it's not a leaf)
        assert_eq!(db.count_descendant_experiments("a").unwrap(), 3);
        assert_eq!(db.count_descendant_experiments("a/d").unwrap(), 1);
        assert_eq!(db.count_descendant_experiments("a/b").unwrap(), 0);
    }

    #[test]
    fn test_count_runs_for_subtree() {
        let db = test_db();
        assert_eq!(db.count_runs_for_subtree("a").unwrap(), 4);
        assert_eq!(db.count_runs_for_subtree("a/b").unwrap(), 2);
        assert_eq!(db.count_runs_for_subtree("a/d").unwrap(), 1);
    }

    #[test]
    fn test_runs_by_status_for_subtree() {
        let db = test_db();
        let mut status = db.runs_by_status_for_subtree("a").unwrap();
        status.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            status,
            vec![
                ("completed".to_string(), 2),
                ("failed".to_string(), 1),
                ("running".to_string(), 1),
            ]
        );
    }

    #[test]
    fn test_run_counts_by_child() {
        let db = test_db();
        let counts = db.run_counts_by_child("e_a").unwrap();
        assert_eq!(
            counts,
            vec![
                ("b".to_string(), 2),
                ("c".to_string(), 1),
                ("d".to_string(), 1),
            ]
        );
    }

    #[test]
    fn test_aggregate_final_metrics() {
        let db = test_db();
        let agg = db.aggregate_final_metrics("e_b").unwrap();
        assert_eq!(agg.len(), 2);
        let acc = agg.iter().find(|m| m.name == "accuracy").unwrap();
        assert!((acc.mean - 0.75).abs() < 0.001);
        assert!((acc.min - 0.65).abs() < 0.001);
        assert!((acc.max - 0.85).abs() < 0.001);
        assert_eq!(acc.count, 2);
        // std_dev for accuracy: values [0.85, 0.65], mean=0.75, std=0.1
        assert!((acc.std_dev - 0.1).abs() < 0.001);
        let loss = agg.iter().find(|m| m.name == "loss").unwrap();
        assert!((loss.mean - 0.45).abs() < 0.001);
        // std_dev for loss: values [0.3, 0.6], mean=0.45, std=0.15
        assert!((loss.std_dev - 0.15).abs() < 0.001);
    }

    #[test]
    fn test_count_unique_configs() {
        let db = test_db();
        assert_eq!(db.count_unique_configs("e_b").unwrap(), 2);
        assert_eq!(db.count_unique_configs("e_c").unwrap(), 1);
    }

    #[test]
    fn test_list_hierarchy() {
        let db = test_db();
        let levels = db.list_hierarchy().unwrap();
        assert_eq!(levels, vec!["benchmark", "method", "variant"]);
    }

    #[test]
    fn test_list_run_params() {
        let db = test_db();
        let params = db.list_run_params("r1").unwrap();
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "arch");
        assert_eq!(params[0].value, "resnet18");
        assert_eq!(params[1].name, "fisher_label");
        assert_eq!(params[1].value, "empirical");

        // Run with no params
        let params = db.list_run_params("r2").unwrap();
        assert!(params.is_empty());
    }

    #[test]
    fn test_child_best_metrics() {
        let db = test_db();
        // Parent e_a has children: b (runs r1,r2), c (run r3), d (subtree run r4)
        let data = db.child_best_metrics("e_a").unwrap();
        // Should have entries for (b, accuracy), (b, loss), (c, accuracy), (c, loss),
        // (d, accuracy), (d, loss)
        assert_eq!(data.len(), 6);

        // Child "b" accuracy: r1=0.85, r2=0.65 → min=0.65, max=0.85
        let b_acc = data
            .iter()
            .find(|(n, m, _, _)| n == "b" && m == "accuracy")
            .unwrap();
        assert!((b_acc.2 - 0.65).abs() < 0.001); // min
        assert!((b_acc.3 - 0.85).abs() < 0.001); // max

        // Child "d" loss: r4=0.9 → min=max=0.9
        let d_loss = data
            .iter()
            .find(|(n, m, _, _)| n == "d" && m == "loss")
            .unwrap();
        assert!((d_loss.2 - 0.9).abs() < 0.001);
        assert!((d_loss.3 - 0.9).abs() < 0.001);
    }

    #[test]
    fn test_list_all_lineage() {
        let db = test_db();
        db.conn.execute_batch(
            "INSERT INTO lineage (parent_type, parent_id, child_type, child_id, relation) \
             VALUES ('run', 'r1', 'model', 'm1', 'produced');
             INSERT INTO lineage (parent_type, parent_id, child_type, child_id, relation) \
             VALUES ('model', 'm1', 'model', 'm2', 'fine_tuned');",
        ).unwrap();
        let edges = db.list_all_lineage().unwrap();
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0].relation, "produced");
        assert_eq!(edges[1].relation, "fine_tuned");
    }

    #[test]
    fn test_list_models() {
        let db = test_db();
        db.conn.execute_batch(
            "INSERT INTO models (id, name, version, run_id, artifact_path, framework) \
             VALUES ('m1', 'test-model', '1.0', 'r1', 'path/to/model', 'pytorch');
             INSERT INTO models (id, name, version, run_id, artifact_path, framework) \
             VALUES ('m2', 'test-model', '2.0', 'r2', 'path/to/model2', 'pytorch');",
        ).unwrap();
        let models = db.list_models().unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].name, "test-model");
        assert_eq!(models[0].version, "1.0");
        assert_eq!(models[1].version, "2.0");
    }

    #[test]
    fn test_list_todos() {
        let db = test_db();
        db.conn.execute_batch(
            "INSERT INTO todos (id, scope_type, content, done, priority) \
             VALUES ('t1', 'global', 'First todo', 0, 2);
             INSERT INTO todos (id, scope_type, content, done, priority) \
             VALUES ('t2', 'global', 'Second todo', 1, 1);
             INSERT INTO todos (id, scope_type, scope_id, content, done, priority) \
             VALUES ('t3', 'experiment', 'e_b', 'Exp todo', 0, 0);",
        ).unwrap();
        let all = db.list_todos(None, None).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].id, "t1");
        let global = db.list_todos(Some("global"), None).unwrap();
        assert_eq!(global.len(), 2);
        let exp = db.list_todos(Some("experiment"), Some("e_b")).unwrap();
        assert_eq!(exp.len(), 1);
        assert_eq!(exp[0].content, "Exp todo");
    }

    #[test]
    fn test_data_version_increments_on_external_write() {
        // Use a temp file (not in-memory) so a second connection can see writes.
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path();

        // Initialize the schema via a writable connection.
        let writer = Connection::open(path).unwrap();
        writer
            .execute_batch(include_str!("../../schema/migrations/001_init.sql"))
            .unwrap();

        // Open the read-only Db (the same way the TUI does).
        let db = Db::open(path).unwrap();
        let v1 = db.data_version().unwrap();

        // Write something via the other connection.
        writer
            .execute(
                "INSERT INTO experiments (id, path, name, node_type) VALUES ('e1', 'a', 'a', 'benchmark')",
                [],
            )
            .unwrap();

        let v2 = db.data_version().unwrap();
        assert!(v2 > v1, "data_version should increment after external write (v1={v1}, v2={v2})");

        // Same connection, no changes — should NOT tick again.
        let v3 = db.data_version().unwrap();
        assert_eq!(v2, v3);
    }

    #[test]
    fn test_list_curve_names_returns_distinct_sorted() {
        let db = test_db();
        let names = db.list_curve_names("r1").unwrap();
        assert_eq!(names, vec!["lr_schedule".to_string(), "train_loss".to_string()]);
    }

    #[test]
    fn test_list_curve_names_empty_for_run_with_no_curves() {
        let db = test_db();
        let names = db.list_curve_names("r3").unwrap();
        assert!(names.is_empty());
    }

    #[test]
    fn test_list_curve_points_returns_step_ordered_rows() {
        let db = test_db();
        let points = db.list_curve_points("r1", "train_loss").unwrap();
        assert_eq!(points.len(), 3);
        assert_eq!(points[0], (0, 1.0, Some(0.0)));
        assert_eq!(points[1], (1, 0.8, Some(0.5)));
        assert_eq!(points[2], (2, 0.6, Some(1.0)));
    }

    #[test]
    fn test_list_curve_points_empty_for_unknown_metric() {
        let db = test_db();
        let points = db.list_curve_points("r1", "nonexistent").unwrap();
        assert!(points.is_empty());
    }

    #[test]
    fn test_delete_run_with_curve_points_succeeds() {
        // Regression: ensure delete_run also clears curve_points (FK on run_id).
        // The seed data has 5 curve_points rows for r1 (3 train_loss + 2 lr_schedule).
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path();

        // Initialize schema and seed data via a writable connection.
        let writer = Connection::open(path).unwrap();
        writer
            .execute_batch(include_str!("../../schema/migrations/001_init.sql"))
            .unwrap();
        writer.execute_batch(
            "INSERT INTO experiments VALUES ('e1', 'a', 'a', NULL, '2026-01-01T00:00:00Z', NULL, 'active', 'benchmark');
             INSERT INTO runs VALUES ('r1', 'e1', 'run1', NULL, '2026-01-01T00:00:00Z', NULL, 'running', NULL, NULL, NULL, NULL, NULL);
             INSERT INTO curve_points VALUES ('r1', 'loss', 0, 1.0, 0.0);
             INSERT INTO curve_points VALUES ('r1', 'loss', 1, 0.8, 0.5);"
        ).unwrap();
        drop(writer);

        // delete_run must succeed despite the FK from curve_points → runs.
        Db::delete_run(path, "r1").expect("delete_run should clean up curve_points");

        // Confirm the run and its curve_points are gone.
        let reader = Connection::open(path).unwrap();
        let run_count: i64 = reader.query_row("SELECT COUNT(*) FROM runs WHERE id = 'r1'", [], |r| r.get(0)).unwrap();
        let curve_count: i64 = reader.query_row("SELECT COUNT(*) FROM curve_points WHERE run_id = 'r1'", [], |r| r.get(0)).unwrap();
        assert_eq!(run_count, 0);
        assert_eq!(curve_count, 0);
    }
}
