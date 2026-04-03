use std::path::PathBuf;

use color_eyre::Result;

use crate::db::Db;
use crate::model::{Experiment, Run};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Explorer,
    Detail,
    Compare,
    Diff,
    Registry,
    Lineage,
    TodoGlobal,
}

#[derive(Debug, Clone)]
pub enum Action {
    None,
    Navigate(View),
    Quit,
    Refresh,
}

pub struct AppState {
    pub db: Db,
    pub store_root: PathBuf,
    pub current_view: View,
    pub should_quit: bool,
    pub experiments: Vec<Experiment>,
    pub selected_experiment: Option<usize>,
    pub runs: Vec<Run>,
    pub selected_run: Option<usize>,
    pub selected_runs_for_compare: Vec<String>,
}

impl AppState {
    pub fn new(db: Db, store_root: PathBuf) -> Result<Self> {
        let experiments = db.list_experiments()?;
        Ok(Self {
            db,
            store_root,
            current_view: View::Explorer,
            should_quit: false,
            experiments,
            selected_experiment: None,
            runs: Vec::new(),
            selected_run: None,
            selected_runs_for_compare: Vec::new(),
        })
    }

    pub fn refresh_experiments(&mut self) -> Result<()> {
        self.experiments = self.db.list_experiments()?;
        Ok(())
    }

    pub fn refresh_runs(&mut self) -> Result<()> {
        if let Some(idx) = self.selected_experiment {
            if let Some(exp) = self.experiments.get(idx) {
                self.runs = self.db.list_runs(&exp.id)?;
            }
        }
        Ok(())
    }
}
