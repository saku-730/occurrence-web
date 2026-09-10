import { redirect } from "next/navigation";

export default function NewLabelTemplatePage() {
  // Layout editing now starts from the actual selected occurrences.
  redirect("/occurrences/search");
}
