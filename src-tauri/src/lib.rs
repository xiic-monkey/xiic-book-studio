pub mod adoption;
pub mod ai;
pub mod chapter_memory;
mod commands;
pub mod context_search;
pub mod continuity_ledger;
pub mod db;
pub mod dev_server;
pub mod error;
pub mod gate;
pub mod genre_agent;
pub mod genre_skill;
pub mod index_jobs;
pub mod models;
pub mod quality;
pub mod secrets;
pub mod story_architecture;
pub mod story_index;
pub mod story_search;
pub mod workflow;

use commands::{
    analyze_artifact_quality, analyze_chapter_gate, apply_adoption_proposals, approve_stage,
    check_artifact_ledger_continuity, clear_chapter_history, confirm_story_bible,
    confirm_story_bible_review, create_chapter, create_project, create_targeted_rework,
    delete_artifact, delete_chapter, delete_project, export_project, generate_chapter_split_plan,
    get_project, get_settings, get_story_search_status, list_adoption_proposals, list_artifacts,
    list_models, list_projects, list_story_arcs, list_writing_skills, prepare_artifact_adoptions,
    preview_agent_context, rebuild_chapter_memory, rebuild_story_index, rebuild_story_search_index,
    reject_adoption_proposals, replace_artifact_span, request_revision, retry_index_jobs,
    review_project_continuity, review_story_bible, revise_artifact_span_with_ai, run_agent_step,
    run_story_architect, save_ai_settings, save_foreshadowing, save_knowledge_card,
    save_writing_skill, search_story_context, test_ai_connection, update_adoption_proposal,
    update_chapter, update_project,
};
use db::AppState;
use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let state = AppState::new(app.handle())?;
            state.start_index_worker();
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
            prepare_artifact_adoptions,
            list_adoption_proposals,
            update_adoption_proposal,
            apply_adoption_proposals,
            reject_adoption_proposals,
            test_ai_connection,
            list_models,
            run_agent_step,
            rebuild_chapter_memory,
            rebuild_story_index,
            rebuild_story_search_index,
            retry_index_jobs,
            get_story_search_status,
            run_story_architect,
            create_targeted_rework,
            preview_agent_context,
            confirm_story_bible,
            review_story_bible,
            confirm_story_bible_review,
            list_story_arcs,
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
            check_artifact_ledger_continuity,
            search_story_context
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Xiic Book Studio");
}
