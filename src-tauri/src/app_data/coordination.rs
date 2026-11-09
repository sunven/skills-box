//! Shared by clones of AppPaths: cache workers keep activity guards, cleanup
//! takes exclusivity, and the serialized mutation captures its preferences.
use crate::{error::AppError, settings::DataPreferences};
use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
pub struct Coordination(Mutex<State>);
#[derive(Debug, Default)]
struct State {
    readers: usize,
    cleaning: bool,
    policy: Option<DataPreferences>,
}
pub struct Activity(Arc<Coordination>);
pub struct Cleanup(Arc<Coordination>);
pub struct Operation(Arc<Coordination>);

impl Coordination {
    pub fn activity(self: &Arc<Self>) -> Result<Activity, AppError> {
        let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if state.cleaning {
            return Err(super::busy());
        }
        state.readers += 1;
        Ok(Activity(self.clone()))
    }
    pub fn cleanup(self: &Arc<Self>) -> Result<Cleanup, AppError> {
        let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if state.cleaning || state.readers != 0 {
            return Err(super::busy());
        }
        state.cleaning = true;
        Ok(Cleanup(self.clone()))
    }
    pub fn operation(self: &Arc<Self>, policy: DataPreferences) -> Operation {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).policy = Some(policy);
        Operation(self.clone())
    }
    pub fn policy(&self) -> Option<DataPreferences> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).policy
    }
}
impl Drop for Activity {
    fn drop(&mut self) {
        self.0.0.lock().unwrap_or_else(|p| p.into_inner()).readers -= 1;
    }
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        self.0.0.lock().unwrap_or_else(|p| p.into_inner()).cleaning = false;
    }
}
impl Drop for Operation {
    fn drop(&mut self) {
        self.0.0.lock().unwrap_or_else(|p| p.into_inner()).policy = None;
    }
}
