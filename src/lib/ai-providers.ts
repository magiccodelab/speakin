/** AI provider instance types for multi-provider configuration. */

export interface AiProvider {
  id: string;
  name: string;
  protocol: "openai" | "gemini";
  api_endpoint: string;
  model: string;
  stream: boolean;
  extra_body: Record<string, unknown>;
}

export interface AiProvidersFile {
  providers: AiProvider[];
}
