-- Add migration script here
CREATE TABLE IF NOT EXISTS `timed_fields` (
	`timed_field_id` INT NOT NULL PRIMARY KEY,
	`str_field` VARCHAR(255) NOT NULL,
	`created_at` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
	`updated_at` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
	`deleted_at` TIMESTAMP NULL
);