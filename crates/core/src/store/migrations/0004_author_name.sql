-- Author name from imported archives that could not be mapped to a user.
ALTER TABLE messages ADD COLUMN author_name TEXT;
