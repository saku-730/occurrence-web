use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, PartialEq, ToSchema)]
pub struct OccurrenceMapFeatureCollection {
    #[serde(rename = "type")]
    pub kind: String,
    pub features: Vec<OccurrenceMapFeature>,
}

#[derive(Debug, Serialize, PartialEq, ToSchema)]
pub struct OccurrenceMapFeature {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    pub geometry: OccurrenceMapGeometry,
    pub properties: OccurrenceMapProperties,
}

#[derive(Debug, Serialize, PartialEq, ToSchema)]
pub struct OccurrenceMapGeometry {
    #[serde(rename = "type")]
    pub kind: String,
    /// GeoJSON order: longitude, latitude.
    pub coordinates: Vec<f64>,
}

#[derive(Debug, Serialize, PartialEq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OccurrenceMapProperties {
    pub occurrence_id: String,
    pub occurrence_uri: String,
    pub scientific_name: Option<String>,
    pub event_date: Option<String>,
    pub locality: Option<String>,
    pub municipality: Option<String>,
    pub county: Option<String>,
    pub state_province: Option<String>,
    pub country: Option<String>,
    pub coordinate_source: String,
}
