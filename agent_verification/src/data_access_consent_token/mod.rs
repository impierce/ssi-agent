pub mod aggregate;
pub mod application;
pub mod command;
pub mod error;
pub mod event;
pub mod queries;
pub mod views;

// TODO: The actual aggregate should probably be in agent_holder, but for now we keep it in agent_verification since all the logic necessary to redeem and verify a token lives here too and interdomain communication still needs to be improved.
