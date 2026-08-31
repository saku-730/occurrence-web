import { PaperOccurrenceRegisterClient } from "./paper-occurrence-register-client";

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

  return (
    <PaperOccurrenceRegisterClient
      paperId={params.paperId ?? ""}
      scientificName={params.scientificName ?? ""}
      locality={params.locality ?? ""}
      decimalLatitude={params.decimalLatitude ?? ""}
      decimalLongitude={params.decimalLongitude ?? ""}
    />
  );
}
