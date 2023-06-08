-- Add migration script here
CREATE TABLE IF NOT EXISTS records (
	record_id INT AUTO_INCREMENT PRIMARY KEY,
	str_field VARCHAR(255) NOT NULL,
	updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
);