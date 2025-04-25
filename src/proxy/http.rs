use crate::error::Result;
use crate::Error;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

/// HTTP response helper functions
pub struct HttpResponse;

impl HttpResponse {
    /// Send a 200 OK response with JSON content
    pub async fn send_json_response(
        writer: &mut tokio::io::WriteHalf<TcpStream>,
        json: &str,
    ) -> Result<()> {
        let response = format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: application/json\r\n\
             Access-Control-Allow-Origin: *\r\n\
             Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
             Access-Control-Allow-Headers: Content-Type, Authorization\r\n\
             Content-Length: {}\r\n\
             \r\n\
             {}", 
            json.len(),
            json
        );
        
        writer.write_all(response.as_bytes()).await.map_err(|e| {
            Error::Communication(format!("Failed to send JSON response: {}", e))
        })?;
        
        Ok(())
    }
    
    /// Send a 400 Bad Request response with a message
    pub async fn send_bad_request_response(
        writer: &mut tokio::io::WriteHalf<TcpStream>,
        message: &str,
    ) -> Result<()> {
        Self::send_error_response(writer, 400, message).await
    }
    
    /// Send a 401 Unauthorized response
    pub async fn send_unauthorized_response(
        writer: &mut tokio::io::WriteHalf<TcpStream>,
    ) -> Result<()> {
        let response = format!(
            "HTTP/1.1 401 Unauthorized\r\n\
             Content-Type: application/json\r\n\
             WWW-Authenticate: Bearer\r\n\
             Access-Control-Allow-Origin: *\r\n\
             Content-Length: 26\r\n\
             \r\n\
             {{\"error\":\"Unauthorized\"}}"
        );
        
        writer.write_all(response.as_bytes()).await.map_err(|e| {
            Error::Communication(format!("Failed to send unauthorized response: {}", e))
        })?;
        
        Ok(())
    }
    
    /// Send a 403 Forbidden response with a message
    pub async fn send_forbidden_response(
        writer: &mut tokio::io::WriteHalf<TcpStream>,
        message: &str,
    ) -> Result<()> {
        Self::send_error_response(writer, 403, message).await
    }
    
    /// Send a 404 Not Found response
    pub async fn send_not_found_response(
        writer: &mut tokio::io::WriteHalf<TcpStream>,
    ) -> Result<()> {
        let response = format!(
            "HTTP/1.1 404 Not Found\r\n\
             Content-Type: application/json\r\n\
             Access-Control-Allow-Origin: *\r\n\
             Content-Length: 23\r\n\
             \r\n\
             {{\"error\":\"Not found\"}}"
        );
        
        writer.write_all(response.as_bytes()).await.map_err(|e| {
            Error::Communication(format!("Failed to send not found response: {}", e))
        })?;
        
        Ok(())
    }
    
    /// Send a custom error response with status code and message
    pub async fn send_error_response(
        writer: &mut tokio::io::WriteHalf<TcpStream>,
        status_code: u16,
        message: &str,
    ) -> Result<()> {
        let status_text = match status_code {
            400 => "Bad Request",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            500 => "Internal Server Error",
            _ => "Error",
        };
        
        let escaped_message = message.replace('"', "\\\"");
        let json = format!("{{\"error\":\"{}\"}}", escaped_message);
        
        let response = format!(
            "HTTP/1.1 {} {}\r\n\
             Content-Type: application/json\r\n\
             Access-Control-Allow-Origin: *\r\n\
             Content-Length: {}\r\n\
             \r\n\
             {}", 
            status_code, status_text, json.len(), json
        );
        
        writer.write_all(response.as_bytes()).await.map_err(|e| {
            Error::Communication(format!("Failed to send error response: {}", e))
        })?;
        
        Ok(())
    }
    
    /// Handle an OPTIONS request for CORS
    pub async fn handle_options_request(
        writer: &mut tokio::io::WriteHalf<TcpStream>,
    ) -> Result<()> {
        let response = 
            "HTTP/1.1 204 No Content\r\n\
             Access-Control-Allow-Origin: *\r\n\
             Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
             Access-Control-Allow-Headers: Content-Type, Authorization\r\n\
             Access-Control-Max-Age: 86400\r\n\
             Content-Length: 0\r\n\
             \r\n";
        
        writer.write_all(response.as_bytes()).await.map_err(|e| {
            Error::Communication(format!("Failed to send CORS response: {}", e))
        })?;
        
        Ok(())
    }
}