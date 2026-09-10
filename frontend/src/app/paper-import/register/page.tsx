import { redirect } from "next/navigation";

type SearchParams = Promise<{
  paperId?: string;
  scientificName?: string;
  locality?: string;
  decimalLatitude?: string;
  decimalLongitude?: string;
}>;

export default async function PaperOccurrenceRegisterPage({
  searchParams,
}: {
  searchParams: SearchParams;
}) {
  const params = await searchParams;
  const target = new URLSearchParams();

  if (params.paperId) target.set("paperId", params.paperId);
  if (params.scientificName) target.set("scientificName", params.scientificName);
  if (params.locality) target.set("locality", params.locality);
  if (params.decimalLatitude) target.set("decimalLatitude", params.decimalLatitude);
  if (params.decimalLongitude) target.set("decimalLongitude", params.decimalLongitude);

  redirect(`/occurrences/new?${target.toString()}`);
}
