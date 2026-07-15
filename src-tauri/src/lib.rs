pub mod ai;
mod commands;
pub mod db;
pub mod dev_server;
pub mod error;
pub mod gate;
pub mod genre_skill;
pub mod models;
pub mod quality;
pub mod secrets;
pub mod workflow;

use commands::{
    analyze_artifact_quality, analyze_chapter_gate, approve_stage, clear_chapter_history,
    create_chapter, create_project, delete_artifact, delete_chapter, delete_project,
    export_project, generate_chapter_split_plan, get_project, get_settings, list_artifacts,
    list_models, list_projects, list_writing_skills, replace_artifact_span, request_revision,
    review_project_continuity, revise_artifact_span_with_ai, run_agent_step, save_ai_settings,
    save_foreshadowing, save_knowledge_card, save_writing_skill, search_story_context,
    test_ai_connection, update_chapter, update_project,
};
use db::AppState;
use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let state = AppState::new(app.handle())?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            create_project,
            delete_project,
            create_chapter,
            delete_chapter,
            list_projects,
            get_project,
            update_project,
            update_chapter,
            get_settings,
            save_ai_settings,
            list_writing_skills,
            save_writing_skill,
            save_knowledge_card,
            save_foreshadowing,
            test_ai_connection,
            list_models,
            run_agent_step,
            approve_stage,
            request_revision,
            replace_artifact_span,
            revise_artifact_span_with_ai,
            delete_artifact,
            clear_chapter_history,
            list_artifacts,
            export_project,
            analyze_artifact_quality,
            analyze_chapter_gate,
            generate_chapter_split_plan,
            review_project_continuity,
            search_story_context
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Xiic Book Studio");
}
