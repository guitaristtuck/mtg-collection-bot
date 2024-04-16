use serenity::model::application::ResolvedOption;
use serenity::builder::{CreateCommand, EditInteractionResponse};

pub fn run(_options: &[ResolvedOption]) -> EditInteractionResponse {
    EditInteractionResponse::new().content("F3BK bot reporting in, ready for nonsense")
}

pub fn register() -> CreateCommand {
    CreateCommand::new("ping").description("Check to see if the server is alive")
}