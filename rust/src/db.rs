use std::path::Path;

use color_eyre::Result;
use rusqlite::{Connection, params};

use crate::model::{Artifact, Experiment, LineageEdge, Model, Run, ScalarMetric, Todo};

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
            "SELECT id, path, name, parent_id, created_at, metadata, status FROM experiments ORDER BY created_at",
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
            "SELECT id, path, name, parent_id, created_at, metadata, status FROM experiments WHERE id = ?",
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
            })
        };

        let mut experiments = Vec::new();

        if let Some(pid) = parent_id {
            let mut stmt = self.conn.prepare(
                "SELECT id, path, name, parent_id, created_at, metadata, status FROM experiments WHERE parent_id = ? ORDER BY created_at",
            )?;
            let rows = stmt.query_map(params![pid], row_mapper)?;
            for row in rows {
                experiments.push(row?);
            }
        } else {
            let mut stmt = self.conn.prepare(
                "SELECT id, path, name, parent_id, created_at, metadata, status FROM experiments WHERE parent_id IS NULL ORDER BY created_at",
            )?;
            let rows = stmt.query_map([], row_mapper)?;
            for row in rows {
                experiments.push(row?);
            }
        }

        Ok(experiments)
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
