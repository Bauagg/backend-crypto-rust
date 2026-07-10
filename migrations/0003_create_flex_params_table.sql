CREATE TABLE flex_params (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    type_param VARCHAR(255) NOT NULL,
    value_param VARCHAR(255) NOT NULL,
    description TEXT,
    user_id UUID NOT NULL REFERENCES users(id),
    photo_id UUID,
    photo_url TEXT,
    header_id UUID,
    -- otomatis true kalau punya foto, false kalau tidak (lihat services/flex_params/service.rs)
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_by VARCHAR(255) NOT NULL,
    updated_by VARCHAR(255) NOT NULL,
    deleted_by VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ
);
