use std::path::Path;

use color_eyre::Result;
use rusqlite::{Connection, params};

use crate::model::{Artifact, Experiment, LineageEdge, MetricAggregate, Model, Run, ScalarMetric, Todo};

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA query_only=ON; PRAGMA journal_mode=WAL;")?;
        Ok(Self { conn })
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

    pub fn get_children(&self, parent_id: Option<&str>) -> Result<Vec<Experiment>> {
        let row_mapper = |row: &rusqlite::Row| {
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
        };

        let mut experiments = Vec::new();

        if let Some(pid) = parent_id {
            let mut stmt = self.conn.prepare(
                "SELECT id, path, name, parent_id, created_at, metadata, status, node_type FROM experiments WHERE parent_id = ? ORDER BY created_at",
            )?;
            let rows = stmt.query_map(params![pid], row_mapper)?;
            for row in rows {
                experiments.push(row?);
            }
        } else {
            let mut stmt = self.conn.prepare(
                "SELECT id, path, name, parent_id, created_at, metadata, status, node_type FROM experiments WHERE parent_id IS NULL ORDER BY created_at",
            )?;
            let rows = stmt.query_map([], row_mapper)?;
            for row in rows {
                experiments.push(row?);
            }
        }

        Ok(experiments)
    }

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
            "SELECT id, experiment_id, name, config, started_at, ended_at, status, hostname, git_sha, tags, notes FROM runs WHERE experiment_id = ? ORDER BY started_at",
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
            "SELECT id, experiment_id, name, config, started_at, ended_at, status, hostname, git_sha, tags, notes FROM runs WHERE id = ?",
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
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    // Metrics

    pub fn get_scalar_metrics(&self, run_id: &str, name: Option<&str>) -> Result<Vec<ScalarMetric>> {
        let mut metrics = Vec::new();

        if let Some(metric_name) = name {
            let mut stmt = self.conn.prepare(
                "SELECT id, run_id, step, name, value, wall_time FROM scalar_metrics WHERE run_id = ? AND name = ? ORDER BY step",
            )?;
            let rows = stmt.query_map(params![run_id, metric_name], |row| {
                Ok(ScalarMetric {
                    id: row.get(0)?,
                    run_id: row.get(1)?,
                    step: row.get(2)?,
                    name: row.get(3)?,
                    value: row.get(4)?,
                    wall_time: row.get(5)?,
                })
            })?;
            for row in rows {
                metrics.push(row?);
            }
        } else {
            let mut stmt = self.conn.prepare(
                "SELECT id, run_id, step, name, value, wall_time FROM scalar_metrics WHERE run_id = ? ORDER BY name, step",
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
            for row in rows {
                metrics.push(row?);
            }
        }

        Ok(metrics)
    }

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

    // Lineage

    pub fn get_lineage(&self, entity_type: &str, entity_id: &str) -> Result<Vec<LineageEdge>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, parent_type, parent_id, child_type, child_id, relation, metadata, created_at \
             FROM lineage \
             WHERE (parent_type = ? AND parent_id = ?) OR (child_type = ? AND child_id = ?) \
             ORDER BY created_at",
        )?;
        let rows = stmt.query_map(params![entity_type, entity_id, entity_type, entity_id], |row| {
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

    pub fn recent_runs(&self, limit: i64) -> Result<Vec<Run>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, experiment_id, name, config, started_at, ended_at, status, \
                    hostname, git_sha, tags, notes \
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
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn count_descendant_experiments(&self, path_prefix: &str) -> Result<i64> {
        let pattern = format!("{path_prefix}/%");
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM experiments WHERE path LIKE ?",
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

    pub fn aggregate_final_metrics_for_subtree(
        &self,
        path_prefix: &str,
    ) -> Result<Vec<MetricAggregate>> {
        let pattern = format!("{path_prefix}/%");
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
                 INNER JOIN experiments e ON r.experiment_id = e.id \
                 WHERE e.path = ? OR e.path LIKE ? \
                 GROUP BY sm2.run_id, sm2.name \
             ) latest ON sm.run_id = latest.run_id \
                     AND sm.name = latest.name \
                     AND sm.step = latest.max_step \
             GROUP BY sm.name ORDER BY sm.name",
        )?;
        let rows = stmt.query_map(params![path_prefix, pattern], |row| {
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
             INSERT INTO runs VALUES ('r1', 'e_b', 'run1', '{\"lr\": 0.01}', '2026-01-01T00:00:00Z', '2026-01-01T01:00:00Z', 'completed', NULL, NULL, NULL, NULL);
             INSERT INTO runs VALUES ('r2', 'e_b', 'run2', '{\"lr\": 0.001}', '2026-01-02T00:00:00Z', NULL, 'running', NULL, NULL, NULL, NULL);
             INSERT INTO runs VALUES ('r3', 'e_c', 'run3', '{\"lr\": 0.01}', '2026-01-03T00:00:00Z', '2026-01-03T01:00:00Z', 'completed', NULL, NULL, NULL, NULL);
             INSERT INTO runs VALUES ('r4', 'e_e', 'run4', '{\"lr\": 0.1}', '2026-01-04T00:00:00Z', NULL, 'failed', NULL, NULL, NULL, NULL);
             INSERT INTO scalar_metrics VALUES (NULL, 'r1', 10, 'loss', 0.5, NULL);
             INSERT INTO scalar_metrics VALUES (NULL, 'r1', 20, 'loss', 0.3, NULL);
             INSERT INTO scalar_metrics VALUES (NULL, 'r1', 10, 'accuracy', 0.7, NULL);
             INSERT INTO scalar_metrics VALUES (NULL, 'r1', 20, 'accuracy', 0.85, NULL);
             INSERT INTO scalar_metrics VALUES (NULL, 'r2', 5, 'loss', 0.6, NULL);
             INSERT INTO scalar_metrics VALUES (NULL, 'r2', 5, 'accuracy', 0.65, NULL);
             INSERT INTO scalar_metrics VALUES (NULL, 'r3', 15, 'loss', 0.4, NULL);
             INSERT INTO scalar_metrics VALUES (NULL, 'r3', 15, 'accuracy', 0.8, NULL);
             INSERT INTO scalar_metrics VALUES (NULL, 'r4', 10, 'loss', 0.9, NULL);
             INSERT INTO scalar_metrics VALUES (NULL, 'r4', 10, 'accuracy', 0.4, NULL);",
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
        assert_eq!(db.count_descendant_experiments("a").unwrap(), 4);
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
    fn test_aggregate_final_metrics_for_subtree() {
        let db = test_db();
        let agg = db.aggregate_final_metrics_for_subtree("a").unwrap();
        assert_eq!(agg.len(), 2);
        let loss = agg.iter().find(|m| m.name == "loss").unwrap();
        assert!((loss.mean - 0.55).abs() < 0.001);
        assert!((loss.min - 0.3).abs() < 0.001);
        assert!((loss.max - 0.9).abs() < 0.001);
        assert_eq!(loss.count, 4);
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
}
