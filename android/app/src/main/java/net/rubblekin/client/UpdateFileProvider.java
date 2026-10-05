package net.rubblekin.client;

/** FileProvider recommends a subclass for consistent behavior across devices. */
public final class UpdateFileProvider extends androidx.core.content.FileProvider {
    public UpdateFileProvider() { super(); }
}
