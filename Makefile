up:
	docker compose up -d --build --force-recreate --remove-orphans

down:
	docker compose down

logs:
	docker compose logs -f xenovrastream

test:
	cd xenovrastream && cargo test

check:
	cd xenovrastream && cargo check && cargo clippy
