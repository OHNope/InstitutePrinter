.PHONY: clean debug release

clean:
	./scripts/clean.sh

debug:
	./scripts/build-installer.sh "$(TARGET)" debug

release:
	./scripts/build-installer.sh "$(TARGET)" release
