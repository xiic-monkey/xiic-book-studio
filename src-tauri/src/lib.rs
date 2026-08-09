pub mod adoption;
pub mod agent_run_service;
pub mod agent_tools;
pub mod ai;
pub mod application;
pub mod chapter_memory;
mod commands;
pub mod context_search;
pub mod continuity_ledger;
pub mod contracts;
pub mod db;
pub mod dev_server;
pub mod error;
pub mod gate;
pub mod genre_agent;
pub mod genre_skill;
pub mod index_jobs;
pub mod models;
pub mod prompt_templates;
pub mod quality;
pub mod reference;
pub mod secrets;
pub mod story_architecture;
pub mod story_index;
pub mod story_search;
pub mod tool_runtime;
pub mod v2_storage;
pub mod web_search;
pub mod workflow;

use application::ApplicationGateway;
use commands::{
    analyze_artifact_quality, analyze_chapter_gate, apply_action_proposal,
    apply_adoption_proposals, approve_stage, cancel_agent_run, check_artifact_ledger_continuity,
    clear_chapter_history, confirm_story_bible, confirm_story_bible_review, create_chapter,
    create_project, create_targeted_rework, delete_ai_provider, delete_artifact, delete_chapter,
    delete_project, export_project, generate_chapter_split_plan, get_active_agent_run,
    get_agent_run, get_artifact_v2, get_project, get_project_workspace, get_provider_capabilities,
    get_settings, get_story_search_status, import_reference_text, list_action_proposals_v2,
    list_adoption_proposals, list_agent_tools, list_agents, list_ai_providers,
    list_artifact_summaries, list_artifacts, list_index_jobs, list_legacy_agent_prompts,
    list_models, list_projects, list_reference_materials, list_run_events, list_story_arcs,
    list_tool_definitions, list_writing_skills, prepare_artifact_adoptions, preview_agent_context,
    preview_agent_run, rebuild_chapter_memory, rebuild_story_index, rebuild_story_search_index,
    reject_action_proposal, reject_adoption_proposals, remove_reference_material,
    replace_artifact_span, request_revision, rerank_story_context, reset_agent_prompt,
    retry_index_jobs, review_project_continuity, review_story_bible, revise_artifact_span_with_ai,
    run_agent_step, run_story_architect, save_agent_settings, save_ai_provider, save_ai_settings,
    save_foreshadowing, save_knowledge_card, save_writing_skill, search_story_context,
    start_agent_run, start_revision_run, start_story_architect_run, test_ai_connection,
    update_adoption_proposal, update_chapter, update_project, update_reference_material,
};
use db::AppState;
use tauri::{Emitter, Manager};

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let state = AppState::new(app.handle())?;
            let gateway = ApplicationGateway::new(state);
            gateway.start_background_workers();
            let mut run_events = gateway.subscribe_run_events();
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    match run_events.recv().await {
                        Ok(event) => {
                            if let Err(error) = app_handle.emit("agent-run-event", &event) {
                                eprintln!("unable to emit Agent run event: {error}");
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
            app.manage(gateway);
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
            list_ai_providers,
            save_ai_provider,
            delete_ai_provider,
            list_agents,
            list_agent_tools,
            save_agent_settings,
            reset_agent_prompt,
            list_writing_skills,
            save_writing_skill,
            save_knowledge_card,
            save_foreshadowing,
            import_reference_text,
            list_reference_materials,
            update_reference_material,
            remove_reference_material,
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
            search_story_context,
            rerank_story_context,
            list_tool_definitions,
            preview_agent_run,
            start_agent_run,
            start_story_architect_run,
            start_revision_run,
            cancel_agent_run,
            get_agent_run,
            list_run_events,
            get_active_agent_run,
            get_project_workspace,
            get_artifact_v2,
            list_artifact_summaries,
            list_index_jobs,
            list_legacy_agent_prompts,
            get_provider_capabilities,
            list_action_proposals_v2,
            apply_action_proposal,
            reject_action_proposal,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Xiic Book Studio");
}
