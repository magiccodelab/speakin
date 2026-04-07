/** Prompt template types for AI text optimization. */

export interface PromptTemplate {
  id: string;
  name: string;
  category: string;
  system_prompt: string;
  user_prompt_template: string;
  is_builtin: boolean;
}

export interface PromptsFile {
  prompts: PromptTemplate[];
}
