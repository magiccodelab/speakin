export interface TextReplacement {
  from: string;
  to: string;
}

export interface TextReplacementsFile {
  replacements: TextReplacement[];
}
