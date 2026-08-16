import { useEffect, useRef, useState } from "react";
import { X, Save, Loader2 } from "lucide-react";
import type { NewProject } from "../types";
import { Select } from "./Select";

const NEW_PROJECT_FORM_ID = "new-project-form";

interface NewProjectModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSubmit: (project: NewProject) => Promise<void>;
  formData: NewProject;
  onFormChange: (data: NewProject) => void;
  busy: boolean;
}

export function NewProjectModal({ isOpen, onClose, onSubmit, formData, onFormChange, busy }: NewProjectModalProps) {
  const [error, setError] = useState<string | null>(null);
  const formRef = useRef<HTMLFormElement>(null);

  useEffect(() => {
    if (isOpen) {
      setError(null);
      setTimeout(() => formRef.current?.querySelector('input')?.focus(), 0);
    }
  }, [isOpen]);

  if (!isOpen) return null;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
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
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <header className="modal-header">
          <h2>新建书籍</h2>
          <button className="modal-close" onClick={onClose} aria-label="关闭">
            <X size={16} />
          </button>
        </header>
        <form id={NEW_PROJECT_FORM_ID} ref={formRef} onSubmit={handleSubmit} className="modal-body">
          {error && <div className="modal-error">{error}</div>}
          <div className="form-field">
            <label htmlFor="title">书名</label>
            <input
              id="title"
              type="text"
              value={formData.title}
              onChange={(e) => onFormChange({ ...formData, title: e.target.value })}
              placeholder="书名"
              required
            />
          </div>
          <div className="form-field">
            <label htmlFor="genre">题材</label>
            <Select
              id="genre"
              value={formData.genre}
              onChange={(genre) => onFormChange({ ...formData, genre })}
              options={[
                "都市异能", "玄幻", "仙侠", "都市", "科幻", "悬疑", "历史", "游戏", "其他",
              ].map((value) => ({ value, label: value }))}
            />
          </div>
          <div className="form-field">
            <label htmlFor="target_words">预计总字数</label>
            <input
              id="target_words"
              type="number"
              min="10000"
              step="1000"
              value={formData.target_words}
              onChange={(e) => onFormChange({ ...formData, target_words: Number(e.target.value) })}
            />
          </div>
          <div className="form-field">
            <label htmlFor="premise">核心设定 / 梗概</label>
            <textarea
              id="premise"
              rows={4}
              value={formData.premise}
              onChange={(e) => onFormChange({ ...formData, premise: e.target.value })}
              placeholder="一句话梗概"
            />
          </div>
        </form>
        <footer className="modal-footer">
          <button type="button" className="btn-ghost" onClick={onClose} disabled={busy}>
            取消
          </button>
          <button type="submit" form={NEW_PROJECT_FORM_ID} className="btn-primary" disabled={busy}>
            {busy ? <Loader2 size={14} className="spin" /> : <><Save size={14} /> 新建</>}
          </button>
        </footer>
      </div>
    </div>
  );
}
