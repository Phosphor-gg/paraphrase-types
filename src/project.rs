use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProjectResponse {
    pub id: String,
    pub name: String
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteProjectResponse {
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectResponse {
    pub id: String,
    pub name: String
}