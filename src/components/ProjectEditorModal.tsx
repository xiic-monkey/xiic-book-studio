import { useEffect, useRef, useState } from "react";
import { Loader2, Save, X } from "lucide-react";
import type { ProjectUpdate } from "../types";

const PROJECT_EDITOR_FORM_ID = "project-editor-form";

interface ProjectEditorModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSubmit: (project: ProjectUpdate) => Promise<void>;
  formData: ProjectUpdate;
  onFormChange: (data: ProjectUpdate) => void;
  busy: boolean;
}

export function ProjectEditorModal({
  isOpen,
  onClose,
  onSubmit,
  formData,
  onFormChange,
  busy,
}: ProjectEditorModalProps) {
  const [error, setError] = useState<string | null>(null);
  const formRef = useRef<HTMLFormElement>(null);

  useEffect(() => {
    if (!isOpen) return;
    setError(null);
    setTimeout(() => formRef.current?.querySelector("input")?.focus(), 0);
  }, [isOpen]);

  if (!isOpen) return null;

  const handleSubmit = async (event: React.FormEvent) => {
    event.preventDefault();
    setError(null);
    try {
      await onSubmit(formData);
      onClose();
    } catch (err) {
      setError(String(err));
    }
  };

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal" onClick={(event) => event.stopPropagation()}>
        <header className="modal-header">
          <h2>编辑书籍</h2>
          <button className="modal-close" onClick={onClose} aria-label="关闭">
            <X size={16} />
          </button>
        </header>
        <form id={PROJECT_EDITOR_FORM_ID} ref={formRef} onSubmit={handleSubmit} className="modal-body">
          {error && <div className="modal-error">{error}</div>}
          <div className="form-field">
            <label htmlFor="project_title">书名</label>
            <input
              id="project_title"
              type="text"
              value={formData.title}
              onChange={(event) => onFormChange({ ...formData, title: event.target.value })}
              placeholder="小说标题"
              required
            />
          </div>
          <div className="form-field">
            <label htmlFor="project_genre">题材</label>
            <input
              id="project_genre"
              type="text"
              value={formData.genre}
              onChange={(event) => onFormChange({ ...formData, genre: event.target.value })}
              placeholder="例如：都市异能 / 玄幻 / 科幻"
              required
            />
          </div>
          <div className="form-field">
            <label htmlFor="project_target_words">预计总字数</label>
            <input
              id="project_target_words"
              type="number"
              min="10000"
              step="10000"
              value={formData.target_words}
              onChange={(event) =>
                onFormChange({ ...formData, target_words: Number(event.target.value) || 0 })
              }
            />
          </div>
          <div className="form-field">
            <label htmlFor="project_status">项目状态</label>
            <select
              id="project_status"
              value={formData.status}
              onChange={(event) => onFormChange({ ...formData, status: event.target.value })}
            >
              <option value="active">进行中</option>
              <option value="paused">暂停</option>
              <option value="archived">归档</option>
            </select>
          </div>
          <div className="form-field">
            <label htmlFor="project_premise">核心设定 / 梗概</label>
            <textarea
              id="project_premise"
              rows={5}
              value={formData.premise}
              onChange={(event) => onFormChange({ ...formData, premise: event.target.value })}
              placeholder="一句话概括故事引擎、核心冲突、卖点和写作方向..."
            />
          </div>
        </form>
        <footer className="modal-footer">
          <button type="button" className="btn-ghost" onClick={onClose} disabled={busy}>
            取消
          </button>
          <button type="submit" form={PROJECT_EDITOR_FORM_ID} className="btn-primary" disabled={busy}>
            {busy ? <Loader2 size={14} className="spin" /> : <><Save size={14} /> 保存</>}
          </button>
        </footer>
      </div>
    </div>
  );
}
