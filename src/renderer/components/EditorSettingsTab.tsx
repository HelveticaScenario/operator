import React, { useMemo, useState } from 'react';
import Form, { type IChangeEvent } from '@rjsf/core';
import type { RJSFSchema, UiSchema } from '@rjsf/utils';
import validator from '@rjsf/validator-ajv8';
import type {
    AppConfig,
    BundledFont,
    MonospaceFont,
    SystemFont,
} from '../../shared/ipcTypes';
import type { AppTheme } from '../themes/types';
import { configSchema } from '../configSchema';

const BUNDLED_FONTS: BundledFont[] = [
    'Fira Code',
    'JetBrains Mono',
    'Cascadia Code',
    'Source Code Pro',
    'IBM Plex Mono',
    'Hack',
    'Inconsolata',
    'Monaspace Neon',
    'Monaspace Argon',
    'Monaspace Xenon',
    'Monaspace Krypton',
    'Monaspace Radon',
    'Geist Mono',
    'Iosevka',
    'Victor Mono',
    'Roboto Mono',
    'Maple Mono',
    'Commit Mono',
    '0xProto',
    'Intel One Mono',
    'Mononoki',
    'Anonymous Pro',
    'Recursive',
];

const SYSTEM_FONTS: SystemFont[] = ['SF Mono', 'Monaco', 'Menlo', 'Consolas'];

interface EditorFormData {
    theme?: string;
    font?: MonospaceFont;
    fontSize?: number;
    fontLigatures?: boolean;
    cursorStyle?: AppConfig['cursorStyle'];
}

interface EditorSettingsTabProps {
    config: AppConfig;
    themes: AppTheme[];
    onConfigChange: (partial: Partial<AppConfig>) => void;
}

function isFontInstalled(fontName: string): boolean {
    const canvas = document.createElement('canvas');
    const ctx = canvas.getContext('2d');
    if (!ctx) {
        return false;
    }

    const testString = 'mmmmmmmmmmlli1WWW@#$';
    const baselines = ['monospace', 'sans-serif', 'serif'] as const;
    const size = '72px';

    for (const baseline of baselines) {
        ctx.font = `${size} ${baseline}`;
        const baselineWidth = ctx.measureText(testString).width;

        ctx.font = `${size} "${fontName}", ${baseline}`;
        const candidateWidth = ctx.measureText(testString).width;

        if (candidateWidth !== baselineWidth) {
            return true;
        }
    }
    return false;
}

const uiSchema: UiSchema = {
    'ui:submitButtonOptions': { norender: true },
    fontSize: { 'ui:widget': 'range' },
};

export function EditorSettingsTab({
    config,
    themes,
    onConfigChange,
}: EditorSettingsTabProps) {
    const [availableSystemFonts] = useState<SystemFont[]>(() =>
        SYSTEM_FONTS.filter(isFontInstalled),
    );

    const schema: RJSFSchema = useMemo(() => {
        const base = configSchema.properties;
        const fontEnum: MonospaceFont[] = [
            ...BUNDLED_FONTS,
            ...availableSystemFonts,
        ];
        return {
            type: 'object',
            properties: {
                theme: {
                    type: 'string',
                    title: 'Color Theme',
                    description: base.theme.description,
                    default: base.theme.default,
                    oneOf: themes.map((t) => ({
                        const: t.id,
                        title: t.name,
                    })),
                },
                font: {
                    type: 'string',
                    title: 'Font',
                    description: base.font.description,
                    default: base.font.default,
                    enum: fontEnum,
                },
                fontSize: {
                    type: 'number',
                    title: 'Font Size',
                    description: base.fontSize.description,
                    default: base.fontSize.default,
                    minimum: base.fontSize.minimum,
                    maximum: base.fontSize.maximum,
                },
                fontLigatures: {
                    type: 'boolean',
                    title: 'Font Ligatures',
                    description: base.fontLigatures.description,
                    default: base.fontLigatures.default,
                },
                cursorStyle: {
                    type: 'string',
                    title: 'Cursor Style',
                    description: base.cursorStyle.description,
                    default: base.cursorStyle.default,
                    enum: base.cursorStyle.enum,
                },
            },
        };
    }, [themes, availableSystemFonts]);

    const formData: EditorFormData = {
        theme: config.theme,
        font: config.font,
        fontSize: config.fontSize,
        fontLigatures: config.fontLigatures,
        cursorStyle: config.cursorStyle,
    };

    const handleChange = (e: IChangeEvent) => {
        const data: EditorFormData = e.formData ?? {};
        onConfigChange({
            theme: data.theme,
            font: data.font,
            fontSize: data.fontSize,
            fontLigatures: data.fontLigatures,
            cursorStyle: data.cursorStyle,
        });
    };

    return (
        <div className="settings-tab-content settings-rjsf">
            <Form
                schema={schema}
                uiSchema={uiSchema}
                validator={validator}
                formData={formData}
                onChange={handleChange}
                liveValidate={false}
                showErrorList={false}
            />
        </div>
    );
}
