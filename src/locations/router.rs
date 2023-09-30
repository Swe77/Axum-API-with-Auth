pub mod router {
    use serde_json::{json, Value};
    use axum::{
        Router, http::StatusCode, Json, response::IntoResponse, extract::State, extract,
    };
    use http::HeaderMap;
    use crate::{
        common::db::ConnectionPool,
        locations:: {
            service::service::LocationDatabase as locationsDB,
            model::UpsertLocation
        },
        common::security:: {enforce_role_policy, decode_claims}
    };
    use crate::users::model::UserRole;

    // - - - - - - - - - - - [ROUTES] - - - - - - - - - - -

    pub fn locations_routes(shared_connection_pool: ConnectionPool) -> Router {
        Router::new()
            .route("/locations", axum::routing::post(create_location_handler))
            .route("/locations/:location_id", axum::routing::get(read_location_handler))
            .route("/locations/:location_id", axum::routing::put(update_location_handler))
            .route("/locations/:location_id", axum::routing::delete(delete_location_handler))
            .with_state(shared_connection_pool)
    }

    // - - - - - - - - - - - [HANDLERS] - - - - - - - - - - -

    pub async fn create_location_handler(
        headers: HeaderMap,
        State(shared_state): State<ConnectionPool>,
        Json(upsert_location): Json<UpsertLocation>,
    ) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {

        // Decode claims from bearer token header
        let claims = match decode_claims(&headers) {
            Ok(claims) => claims,
            Err((status_code, json_value)) => return Err((status_code, json_value)),
        };

        // Ensure that the user derived from claims exists and has the role 'WRITER'
        let authorization = enforce_role_policy(&shared_state, &claims, UserRole::WRITER).await;

        match authorization {
            Ok(_authorized_user) => {
                let connection = shared_state.pool.get()
                    .expect("Failed to acquire connection from pool");

                match locationsDB::new(connection).create(upsert_location) {
                    Ok(new_location) => Ok((StatusCode::CREATED, Json(new_location))),
                    Err(err) => {
                        eprintln!("Error creating location: {:?}", err);
                        Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Failed to create location"}))))
                    }
                }
            }
            Err(err) => Err(err),
        }
    }

    pub async fn read_location_handler(
        State(shared_state): State<ConnectionPool>,
        path: extract::Path<(i32,)>,
    ) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
        let (location_id,) = path.0;

        let connection = shared_state.pool.get()
            .expect("Failed to acquire connection from pool");

        let mut locations = locationsDB::new(connection);

        match locations.get(location_id) {
            Ok(location) => {
                if let Some(location) = location {
                    Ok((StatusCode::OK, Json(location)))
                } else {
                    Err((StatusCode::NOT_FOUND, Json(json!({"error": "Location not found"}))))
                }
            },
            Err(err) => {
                eprintln!("Error reading location: {:?}", err);
                Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Failed to read location"}))))
            }
        }
    }

    pub async fn update_location_handler(
        State(shared_state): State<ConnectionPool>,
        path: extract::Path<(i32,)>,
        Json(upsert_location): Json<UpsertLocation>,
    ) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
        let (location_id,) = path.0;

        let connection = shared_state.pool.get()
            .expect("Failed to acquire connection from pool");

        let mut locations = locationsDB::new(connection);

        match locations.update(location_id, upsert_location) {
            Ok(updated_location) => Ok((StatusCode::OK, Json(updated_location))),
            Err(diesel::result::Error::NotFound) => {
                Err((StatusCode::NOT_FOUND, Json(json!({"error": "Location not found"}))))
            },
            Err(err) => {
                eprintln!("Error updating location: {:?}", err);
                Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Failed to update location"}))))
            }
        }
    }

    pub async fn delete_location_handler(
        State(shared_state): State<ConnectionPool>,
        path: extract::Path<(i32,)>,
    ) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
        let (location_id,) = path.0;

        let connection = shared_state.pool.get()
            .expect("Failed to acquire connection from pool");

        let mut locations = locationsDB::new(connection);

        match locations.delete(location_id) {
            Ok(_) => Ok((StatusCode::NO_CONTENT, ())),
            Err(err) => {
                eprintln!("Error deleting location: {:?}", err);
                Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Failed to delete location"}))))
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use axum::{
            body::Body,
            http::{Request, StatusCode}
        };
        use http::HeaderMap;
        use serde_json::json;
        use tower::ServiceExt;
        use crate::{
            common::{
                db::create_shared_connection_pool,
                util::load_environment_variable
            },
            locations::{
                model::UpsertLocation,
                service::service::LocationDatabase
            },
            locations_routes
        };

        #[tokio::test]
        async fn post_locations_returns_201_on_valid_data() {
            let database_url = load_environment_variable("TEST_DB");
            let connection_pool = create_shared_connection_pool(database_url, 1);
            let service = locations_routes(connection_pool);

            // Data
            let request_body = UpsertLocation {
                star_system: "Fountain".to_string(),
                area: "The Serpent's Lair".to_string(),
            };

            // Create a request with the above data as payload
            let request = Request::builder()
                .uri("/locations")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&request_body).unwrap()))
                .unwrap();

            // Send the request through the service
            let response = service
                .oneshot(request)
                .await
                .unwrap();

            // Assert that the response status is 201
            assert_eq!(response.status(), StatusCode::CREATED);
        }

        #[tokio::test]
        async fn put_locations_returns_200_on_valid_data() {
            let database_url = load_environment_variable("TEST_DB");
            let connection_pool = create_shared_connection_pool(database_url, 2);
            let connection = connection_pool.pool.get().expect("Failed to get connection");
            let mut location_db = LocationDatabase::new(connection);
            let service = locations_routes(connection_pool);

            let request_body = UpsertLocation {
                star_system: "Fountain".to_string(),
                area: "The Serpent's Lair".to_string(),
            };

            // Create a new location with the above data
            let created_location = location_db.create(request_body.clone()).expect("Create location failed");

            // Assert equality
            assert_eq!(request_body.star_system, created_location.star_system);
            assert_eq!(request_body.area, created_location.area);

            let updated_request_body = UpsertLocation {
                star_system: "Kador".to_string(),
                area: "The Crimson Expanse".to_string(),
            };

            // Create a request with the above data as payload
            let request = Request::builder()
                .uri(format!("/locations/{}", created_location.id))
                .method("PUT")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&updated_request_body).unwrap()))
                .unwrap();

            // Send the request through the service
            let response = service
                .oneshot(request)
                .await
                .unwrap();

            // Assert that the response status is 200
            assert_eq!(response.status(), StatusCode::OK);

            // Extract body from response
            let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
            let response_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

            // Construct JSON consisting of expected payload
            let expected_response = json!({
                "id": created_location.id,
                "area": updated_request_body.area,
                "star_system": updated_request_body.star_system
            });

            // Assert equality
            assert_eq!(response_json, expected_response);
        }

        #[tokio::test]
        async fn get_locations_returns_200_on_existing_id() {
            let database_url = load_environment_variable("TEST_DB");
            let connection_pool = create_shared_connection_pool(database_url, 2);
            let connection = connection_pool.pool.get().expect("Failed to get connection");
            let mut location_db = LocationDatabase::new(connection);
            let service = locations_routes(connection_pool);

            let request_body = UpsertLocation {
                star_system: "Fountain".to_string(),
                area: "The Serpent's Lair".to_string(),
            };

            // Create a new location with the above data
            let created_location = location_db.create(request_body.clone()).expect("Create location failed");

            // Create a request with the ID associated with our newly inserted row
            let request = Request::builder()
                .uri(format!("/locations/{}", created_location.id))
                .method("GET")
                .body(Body::empty())
                .unwrap();

            // Send the request through the service
            let response = service
                .oneshot(request)
                .await
                .unwrap();

            // Assert that the response status is 200
            assert_eq!(response.status(), StatusCode::OK);

            // Extract body from response
            let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
            let response_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

            // Construct JSON consisting of expected payload
            let expected_response = json!({
                "id": created_location.id,
                "area": request_body.area,
                "star_system": request_body.star_system
            });

            // Assert equality
            assert_eq!(response_json, expected_response);
        }

        #[tokio::test]
        async fn get_locations_returns_404_on_non_existing_id() {
            let database_url = load_environment_variable("TEST_DB");
            let connection_pool = create_shared_connection_pool(database_url, 2);
            let service = locations_routes(connection_pool);

            // Create a request with the aforementioned id
            let request = Request::builder()
                .uri(format!("/locations/{}", -666)) // Use a non-existent ID
                .method("GET")
                .body(Body::empty())
                .unwrap();

            // Send the request through the service
            let response = service
                .oneshot(request)
                .await
                .unwrap();

            // Assert that the response status is 404 as there are no locations associated with the id
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }

        #[tokio::test]
        async fn delete_locations_returns_204() {
            let database_url = load_environment_variable("TEST_DB");
            let connection_pool = create_shared_connection_pool(database_url, 2);
            let connection = connection_pool.pool.get().expect("Failed to get connection");
            let mut location_db = LocationDatabase::new(connection);
            let service = locations_routes(connection_pool);

            let request_body = UpsertLocation {
                star_system: "Fountain".to_string(),
                area: "The Serpent's Lair".to_string(),
            };

            // Create a new location with the above data
            let created_location = location_db.create(request_body.clone()).expect("Create location failed");

            // Create a request with the ID associated with our newly inserted row
            let request = Request::builder()
                .uri(format!("/locations/{}", created_location.id))
                .method("DELETE")
                .body(Body::empty())
                .unwrap();

            // Send the request through the service
            let response = service
                .oneshot(request)
                .await
                .unwrap();

            // Assert that the response status is 204
            assert_eq!(response.status(), StatusCode::NO_CONTENT);

            // Attempt to retrieve the deleted location
            let deleted_location_result = location_db.get(created_location.id);

            // Assert that the Result is Ok (no error)
            assert!(deleted_location_result.is_ok());

            // Extract the Option<Location> from the Ok variant
            let deleted_location = deleted_location_result.unwrap();

            // Assert that the deleted location is None (i.e., it doesn't exist)
            assert!(deleted_location.is_none());
        }
    }
}
