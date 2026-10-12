use serenity::builder::{CreateCommand, EditInteractionResponse};
use serenity::model::application::ResolvedOption;

pub fn run(_options: &[ResolvedOption]) -> EditInteractionResponse {
    EditInteractionResponse::new().content("F3BK bot reporting in, ready for nonsense")
}

pub fn register() -> CreateCommand {
    CreateCommand::new("ping").description("Check to see if the server is alive")
}
