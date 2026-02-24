//! Default pipeline handler implementations for the 5 kernel agent roles.
//!
//! Each handler wraps the role-specific logic and implements [`AgentHandler`].

mod building;
mod evaluation;
mod learning;
mod pre_load;
mod skill_manage;

pub use building::BuildingHandler;
pub use evaluation::EvaluationHandler;
pub use learning::LearningHandler;
pub use pre_load::PreLoadHandler;
pub use skill_manage::SkillManageHandler;
